use crate::error::{Error, ErrorKind};
use activitystreams::iri_string::types::IriString;
use actix_web::{http::header::Date, web::Bytes};
use awc::Client;
use dashmap::DashMap;
use http_signature_normalization_actix::prelude::*;
use rand::thread_rng;
use rsa::{pkcs1v15::SigningKey, RsaPrivateKey};
use sha2::{Digest, Sha256};
use signature::RandomizedSigner;
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
    const fn failure_threshold() -> usize {
        10
    }

    fn failure_wait() -> Duration {
        Duration::from_secs(ONE_DAY)
    }

    fn should_try(&self) -> bool {
        self.failures < Self::failure_threshold()
            || self.last_attempt + Self::failure_wait() < SystemTime::now()
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

fn build_client(user_agent: &str) -> Client {
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
        }
    }

    fn count_err(&self) {
        let count = self.consecutive_errors.fetch_add(1, Ordering::Relaxed);
        if count + 1 >= self.error_limit {
            tracing::warn!("{} consecutive errors, rebuilding http client", count);
            *self.client.borrow_mut() = build_client(&self.user_agent);
            self.reset_err();
        }
    }

    fn reset_err(&self) {
        self.consecutive_errors.swap(0, Ordering::Relaxed);
    }

    #[tracing::instrument(name = "Fetch Json", skip(self))]
    pub(crate) async fn fetch_json<T>(&self, url: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/json").await
    }

    #[tracing::instrument(name = "Fetch Activity+Json", skip(self))]
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
        let parsed_url = url.parse::<IriString>()?;

        if !self.breakers.should_try(&parsed_url) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();

        let client: Client = self.client.borrow().clone();
        let res = client
            .get(url)
            .insert_header(("Accept", accept))
            .insert_header(Date(SystemTime::now().into()))
            .signature(
                self.config.clone(),
                self.key_id.clone(),
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .send()
            .await;

        if res.is_err() {
            self.count_err();
            self.breakers.fail(&parsed_url);
        }

        let mut res = res.map_err(|e| ErrorKind::SendRequest(url.to_string(), e.to_string()))?;

        self.reset_err();

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        tracing::warn!("Response from {}, {}", url, s);
                    }
                }
            }

            self.breakers.fail(&parsed_url);

            return Err(ErrorKind::Status(url.to_string(), res.status()).into());
        }

        self.breakers.succeed(&parsed_url);

        let body = res
            .body()
            .await
            .map_err(|e| ErrorKind::ReceiveResponse(url.to_string(), e.to_string()))?;

        Ok(serde_json::from_slice(body.as_ref())?)
    }

    #[tracing::instrument(name = "Fetch Bytes", skip(self))]
    pub(crate) async fn fetch_bytes(&self, url: &str) -> Result<(String, Bytes), Error> {
        let parsed_url = url.parse::<IriString>()?;

        if !self.breakers.should_try(&parsed_url) {
            return Err(ErrorKind::Breaker.into());
        }

        tracing::info!("Fetching bytes for {}", url);
        let signer = self.signer();

        let client: Client = self.client.borrow().clone();
        let res = client
            .get(url)
            .insert_header(("Accept", "*/*"))
            .insert_header(Date(SystemTime::now().into()))
            .signature(
                self.config.clone(),
                self.key_id.clone(),
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .send()
            .await;

        if res.is_err() {
            self.breakers.fail(&parsed_url);
            self.count_err();
        }

        let mut res = res.map_err(|e| ErrorKind::SendRequest(url.to_string(), e.to_string()))?;

        self.reset_err();

        let content_type = if let Some(content_type) = res.headers().get("content-type") {
            if let Ok(s) = content_type.to_str() {
                s.to_owned()
            } else {
                return Err(ErrorKind::ContentType.into());
            }
        } else {
            return Err(ErrorKind::ContentType.into());
        };

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        tracing::warn!("Response from {}, {}", url, s);
                    }
                }
            }

            self.breakers.fail(&parsed_url);

            return Err(ErrorKind::Status(url.to_string(), res.status()).into());
        }

        self.breakers.succeed(&parsed_url);

        let bytes = match res.body().limit(1024 * 1024 * 4).await {
            Err(e) => {
                return Err(ErrorKind::ReceiveResponse(url.to_string(), e.to_string()).into());
            }
            Ok(bytes) => bytes,
        };

        Ok((content_type, bytes))
    }

    #[tracing::instrument(
        "Deliver to Inbox",
        skip_all,
        fields(inbox = inbox.to_string().as_str())
    )]
    pub(crate) async fn deliver<T>(&self, inbox: IriString, item: &T) -> Result<(), Error>
    where
        T: serde::ser::Serialize + std::fmt::Debug,
    {
        if !self.breakers.should_try(&inbox) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
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
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .split();

        let res = req.send_body(body).await;

        if res.is_err() {
            self.count_err();
            self.breakers.fail(&inbox);
        }

        let mut res = res.map_err(|e| ErrorKind::SendRequest(inbox.to_string(), e.to_string()))?;

        self.reset_err();

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        tracing::warn!("Response from {}, {}", inbox.as_str(), s);
                    }
                }
            }

            self.breakers.fail(&inbox);
            return Err(ErrorKind::Status(inbox.to_string(), res.status()).into());
        }

        self.breakers.succeed(&inbox);

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
        let signature = signing_key.try_sign_with_rng(thread_rng(), signing_string.as_bytes())?;
        Ok(base64::encode(signature.as_ref()))
    }
}
