use crate::{
    data::LastOnline,
    error::{Error, ErrorKind},
};
use activitystreams::iri_string::types::IriString;
use actix_web::http::header::Date;
use awc::{error::SendRequestError, Client, ClientResponse, Connector};
use base64::{engine::general_purpose::STANDARD, Engine};
use dashmap::DashMap;
use http_signature_normalization_actix::prelude::*;
use rand::thread_rng;
use rsa::{
    pkcs1v15::SigningKey,
    sha2::{Digest, Sha256},
    signature::{RandomizedSigner, SignatureEncoding},
    RsaPrivateKey,
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tracing_awc::Tracing;

const ONE_SECOND: u64 = 1;
const ONE_MINUTE: u64 = 60 * ONE_SECOND;
const ONE_HOUR: u64 = 60 * ONE_MINUTE;
const ONE_DAY: u64 = 24 * ONE_HOUR;

#[derive(Clone)]
pub(crate) struct Breakers {
    inner: Arc<DashMap<String, Breaker>>,
}

impl std::fmt::Debug for Breakers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Breakers").finish()
    }
}

impl Breakers {
    fn should_try(&self, url: &IriString) -> bool {
        if let Some(authority) = url.authority_str() {
            if let Some(breaker) = self.inner.get(authority) {
                breaker.should_try()
            } else {
                true
            }
        } else {
            false
        }
    }

    fn fail(&self, url: &IriString) {
        if let Some(authority) = url.authority_str() {
            let should_write = {
                if let Some(mut breaker) = self.inner.get_mut(authority) {
                    breaker.fail();
                    if !breaker.should_try() {
                        tracing::warn!("Failed breaker for {authority}");
                    }
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut breaker = self.inner.entry(authority.to_owned()).or_default();
                breaker.fail();
            }
        }
    }

    fn succeed(&self, url: &IriString) {
        if let Some(authority) = url.authority_str() {
            let should_write = {
                if let Some(mut breaker) = self.inner.get_mut(authority) {
                    breaker.succeed();
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut breaker = self.inner.entry(authority.to_owned()).or_default();
                breaker.succeed();
            }
        }
    }
}

impl Default for Breakers {
    fn default() -> Self {
        Breakers {
            inner: Arc::new(DashMap::new()),
        }
    }
}

#[derive(Debug)]
struct Breaker {
    failures: usize,
    last_attempt: SystemTime,
    last_success: SystemTime,
}

impl Breaker {
    const FAILURE_WAIT: Duration = Duration::from_secs(ONE_DAY);
    const FAILURE_THRESHOLD: usize = 10;

    fn should_try(&self) -> bool {
        self.failures < Self::FAILURE_THRESHOLD
            || self.last_attempt + Self::FAILURE_WAIT < SystemTime::now()
    }

    fn fail(&mut self) {
        self.failures += 1;
        self.last_attempt = SystemTime::now();
    }

    fn succeed(&mut self) {
        self.failures = 0;
        self.last_attempt = SystemTime::now();
        self.last_success = SystemTime::now();
    }
}

impl Default for Breaker {
    fn default() -> Self {
        let now = SystemTime::now();

        Breaker {
            failures: 0,
            last_attempt: now,
            last_success: now,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Requests {
    pool_size: usize,
    client: Client,
    key_id: String,
    user_agent: String,
    private_key: RsaPrivateKey,
    config: Config,
    breakers: Breakers,
    last_online: Arc<LastOnline>,
}

impl std::fmt::Debug for Requests {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Requests")
            .field("pool_size", &self.pool_size)
            .field("key_id", &self.key_id)
            .field("user_agent", &self.user_agent)
            .field("config", &self.config)
            .field("breakers", &self.breakers)
            .finish()
    }
}

thread_local! {
    static CLIENT: std::cell::OnceCell<Client> = std::cell::OnceCell::new();
}

pub(crate) fn build_client(user_agent: &str, pool_size: usize) -> Client {
    CLIENT.with(|client| {
        client
            .get_or_init(|| {
                let connector = Connector::new().limit(pool_size);

                Client::builder()
                    .connector(connector)
                    .wrap(Tracing)
                    .add_default_header(("User-Agent", user_agent.to_string()))
                    .timeout(Duration::from_secs(15))
                    .finish()
            })
            .clone()
    })
}

impl Requests {
    pub(crate) fn new(
        key_id: String,
        private_key: RsaPrivateKey,
        user_agent: String,
        breakers: Breakers,
        last_online: Arc<LastOnline>,
        pool_size: usize,
    ) -> Self {
        Requests {
            pool_size,
            client: build_client(&user_agent, pool_size),
            key_id,
            user_agent,
            private_key,
            config: Config::default().mastodon_compat(),
            breakers,
            last_online,
        }
    }

    pub(crate) fn reset_breaker(&self, iri: &IriString) {
        self.breakers.succeed(iri);
    }

    async fn check_response(
        &self,
        parsed_url: &IriString,
        res: Result<ClientResponse, SendRequestError>,
    ) -> Result<ClientResponse, Error> {
        if res.is_err() {
            self.breakers.fail(&parsed_url);
        }

        let mut res =
            res.map_err(|e| ErrorKind::SendRequest(parsed_url.to_string(), e.to_string()))?;

        if res.status().is_server_error() {
            self.breakers.fail(&parsed_url);

            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        tracing::warn!("Response from {parsed_url}, {s}");
                    }
                }
            }

            return Err(ErrorKind::Status(parsed_url.to_string(), res.status()).into());
        }

        self.last_online.mark_seen(&parsed_url);
        self.breakers.succeed(&parsed_url);

        Ok(res)
    }

    #[tracing::instrument(name = "Fetch Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_json<T>(&self, url: &IriString) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/json").await
    }

    #[tracing::instrument(name = "Fetch Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_json_msky<T>(&self, url: &IriString) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut res = self
            .do_deliver(
                url,
                &serde_json::json!({}),
                "application/json",
                "application/json",
            )
            .await?;

        let body = res
            .body()
            .await
            .map_err(|e| ErrorKind::ReceiveResponse(url.to_string(), e.to_string()))?;

        Ok(serde_json::from_slice(body.as_ref())?)
    }

    #[tracing::instrument(name = "Fetch Activity+Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch<T>(&self, url: &IriString) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/activity+json").await
    }

    async fn do_fetch<T>(&self, url: &IriString, accept: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut res = self.do_fetch_response(url, accept).await?;

        let body = res
            .body()
            .await
            .map_err(|e| ErrorKind::ReceiveResponse(url.to_string(), e.to_string()))?;

        Ok(serde_json::from_slice(body.as_ref())?)
    }

    #[tracing::instrument(name = "Fetch response", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_response(&self, url: &IriString) -> Result<ClientResponse, Error> {
        self.do_fetch_response(url, "*/*").await
    }

    pub(crate) async fn do_fetch_response(
        &self,
        url: &IriString,
        accept: &str,
    ) -> Result<ClientResponse, Error> {
        if !self.breakers.should_try(url) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
        let span = tracing::Span::current();

        let res = self
            .client
            .get(url.as_str())
            .insert_header(("Accept", accept))
            .insert_header(Date(SystemTime::now().into()))
            .no_decompress()
            .signature(
                self.config.clone(),
                self.key_id.clone(),
                move |signing_string| {
                    span.record("signing_string", signing_string);
                    span.in_scope(|| signer.sign(signing_string))
                },
            )
            .await?
            .send()
            .await;

        let res = self.check_response(url, res).await?;

        Ok(res)
    }

    #[tracing::instrument(
        "Deliver to Inbox",
        skip_all,
        fields(inbox = inbox.to_string().as_str(), signing_string)
    )]
    pub(crate) async fn deliver<T>(&self, inbox: &IriString, item: &T) -> Result<(), Error>
    where
        T: serde::ser::Serialize + std::fmt::Debug,
    {
        self.do_deliver(
            inbox,
            item,
            "application/activity+json",
            "application/activity+json",
        )
        .await?;
        Ok(())
    }

    async fn do_deliver<T>(
        &self,
        inbox: &IriString,
        item: &T,
        content_type: &str,
        accept: &str,
    ) -> Result<ClientResponse, Error>
    where
        T: serde::ser::Serialize + std::fmt::Debug,
    {
        if !self.breakers.should_try(&inbox) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
        let span = tracing::Span::current();
        let item_string = serde_json::to_string(item)?;

        let (req, body) = self
            .client
            .post(inbox.as_str())
            .insert_header(("Accept", accept))
            .insert_header(("Content-Type", content_type))
            .insert_header(Date(SystemTime::now().into()))
            .signature_with_digest(
                self.config.clone(),
                self.key_id.clone(),
                Sha256::new(),
                item_string,
                move |signing_string| {
                    span.record("signing_string", signing_string);
                    span.in_scope(|| signer.sign(signing_string))
                },
            )
            .await?
            .split();

        let res = req.send_body(body).await;

        let res = self.check_response(inbox, res).await?;

        Ok(res)
    }

    fn signer(&self) -> Signer {
        Signer {
            private_key: self.private_key.clone(),
        }
    }
}

struct Signer {
    private_key: RsaPrivateKey,
}

impl Signer {
    fn sign(&self, signing_string: &str) -> Result<String, Error> {
        let signing_key = SigningKey::<Sha256>::new(self.private_key.clone());
        let signature =
            signing_key.try_sign_with_rng(&mut thread_rng(), signing_string.as_bytes())?;
        Ok(STANDARD.encode(signature.to_bytes().as_ref()))
    }
}
