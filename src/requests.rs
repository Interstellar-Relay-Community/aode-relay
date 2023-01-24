use crate::{
    data::LastOnline,
    error::{Error, ErrorKind},
};
use activitystreams::iri_string::types::IriString;
use actix_web::http::header::Date;
use awc::{error::SendRequestError, Client, ClientResponse};
use base64::{engine::general_purpose::STANDARD, Engine};
use dashmap::DashMap;
use http_signature_normalization_actix::prelude::*;
use rand::thread_rng;
use rsa::{
    pkcs1v15::SigningKey,
    sha2::{Digest, Sha256},
    signature::RandomizedSigner,
    RsaPrivateKey,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
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
                        tracing::warn!("Failed breaker for {}", authority);
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
    client: Rc<RefCell<Client>>,
    consecutive_errors: Rc<AtomicUsize>,
    error_limit: usize,
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
            .field("error_limit", &self.error_limit)
            .field("key_id", &self.key_id)
            .field("user_agent", &self.user_agent)
            .field("config", &self.config)
            .field("breakers", &self.breakers)
            .finish()
    }
}

pub(crate) fn build_client(user_agent: &str) -> Client {
    Client::builder()
        .wrap(Tracing)
        .add_default_header(("User-Agent", user_agent.to_string()))
        .timeout(Duration::from_secs(15))
        .finish()
}

impl Requests {
    pub(crate) fn new(
        key_id: String,
        private_key: RsaPrivateKey,
        user_agent: String,
        breakers: Breakers,
        last_online: Arc<LastOnline>,
    ) -> Self {
        Requests {
            client: Rc::new(RefCell::new(build_client(&user_agent))),
            consecutive_errors: Rc::new(AtomicUsize::new(0)),
            error_limit: 3,
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

    fn count_err(&self) {
        let count = self.consecutive_errors.fetch_add(1, Ordering::Relaxed);
        if count + 1 >= self.error_limit {
            tracing::warn!("{} consecutive errors, rebuilding http client", count + 1);
            *self.client.borrow_mut() = build_client(&self.user_agent);
            self.reset_err();
        }
    }

    fn reset_err(&self) {
        self.consecutive_errors.swap(0, Ordering::Relaxed);
    }

    async fn check_response(
        &self,
        parsed_url: &IriString,
        res: Result<ClientResponse, SendRequestError>,
    ) -> Result<ClientResponse, Error> {
        if res.is_err() {
            self.count_err();
            self.breakers.fail(&parsed_url);
        }

        let mut res =
            res.map_err(|e| ErrorKind::SendRequest(parsed_url.to_string(), e.to_string()))?;

        self.reset_err();

        if !res.status().is_success() {
            self.breakers.fail(&parsed_url);

            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        tracing::warn!("Response from {}, {}", parsed_url, s);
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
    pub(crate) async fn fetch_json<T>(&self, url: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/json").await
    }

    #[tracing::instrument(name = "Fetch Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_json_msky<T>(&self, url: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch_msky(url, "application/json").await
    }

    #[tracing::instrument(name = "Fetch Activity+Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch<T>(&self, url: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/activity+json").await
    }

    async fn do_fetch<T>(&self, url: &str, accept: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch_inner(url, accept, false).await
    }

    async fn do_fetch_msky<T>(&self, url: &str, accept: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch_inner(url, accept, true).await
    }

    async fn do_fetch_inner<T>(&self, url: &str, accept: &str, use_post: bool) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let parsed_url = url.parse::<IriString>()?;

        if !self.breakers.should_try(&parsed_url) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
        let span = tracing::Span::current();

        let client: Client = self.client.borrow().clone();
        let client_req = match use_post {
            true => client.post(url),
            false => client.get(url),
        };
        let client_signed = client_req
            .insert_header(("Accept", accept))
            .insert_header(Date(SystemTime::now().into()))
            .signature(
                self.config.clone(),
                self.key_id.clone(),
                move |signing_string| {
                    span.record("signing_string", signing_string);
                    span.in_scope(|| signer.sign(signing_string))
                },
            )
            .await?;
        let res = match use_post {
            true => {
                let dummy = serde_json::json!({});
                client_signed.send_json(&dummy)
            }
            false => client_signed.send(),
        }
        .await;

        let mut res = self.check_response(&parsed_url, res).await?;

        let body = res
            .body()
            .await
            .map_err(|e| ErrorKind::ReceiveResponse(url.to_string(), e.to_string()))?;

        Ok(serde_json::from_slice(body.as_ref())?)
    }

    #[tracing::instrument(name = "Fetch response", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_response(&self, url: IriString) -> Result<ClientResponse, Error> {
        if !self.breakers.should_try(&url) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
        let span = tracing::Span::current();

        let client: Client = self.client.borrow().clone();
        let res = client
            .get(url.as_str())
            .insert_header(("Accept", "*/*"))
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

        let res = self.check_response(&url, res).await?;

        Ok(res)
    }

    #[tracing::instrument(
        "Deliver to Inbox",
        skip_all,
        fields(inbox = inbox.to_string().as_str(), signing_string)
    )]
    pub(crate) async fn deliver<T>(&self, inbox: IriString, item: &T) -> Result<(), Error>
    where
        T: serde::ser::Serialize + std::fmt::Debug,
    {
        if !self.breakers.should_try(&inbox) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
        let span = tracing::Span::current();
        let item_string = serde_json::to_string(item)?;

        let client: Client = self.client.borrow().clone();
        let (req, body) = client
            .post(inbox.as_str())
            .insert_header(("Accept", "application/activity+json"))
            .insert_header(("Content-Type", "application/activity+json"))
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

        self.check_response(&inbox, res).await?;

        Ok(())
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
        let signing_key = SigningKey::<Sha256>::new_with_prefix(self.private_key.clone());
        let signature =
            signing_key.try_sign_with_rng(&mut thread_rng(), signing_string.as_bytes())?;
        Ok(STANDARD.encode(signature.as_ref()))
    }
}
