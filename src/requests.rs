use crate::error::{Error, ErrorKind};
use activitystreams::url::Url;
use actix_web::{http::header::Date, web::Bytes};
use awc::Client;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use http_signature_normalization_actix::prelude::*;
use rsa::{hash::Hash, padding::PaddingScheme, RsaPrivateKey};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::SystemTime,
};
use tracing::{debug, info, warn};
use tracing_awc::Propagate;

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
    fn should_try(&self, url: &Url) -> bool {
        if let Some(domain) = url.domain() {
            if let Some(breaker) = self.inner.get(domain) {
                breaker.should_try()
            } else {
                true
            }
        } else {
            false
        }
    }

    fn fail(&self, url: &Url) {
        if let Some(domain) = url.domain() {
            let should_write = {
                if let Some(mut breaker) = self.inner.get_mut(domain) {
                    breaker.fail();
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut breaker = self.inner.entry(domain.to_owned()).or_default();
                breaker.fail();
            }
        }
    }

    fn succeed(&self, url: &Url) {
        if let Some(domain) = url.domain() {
            let should_write = {
                if let Some(mut breaker) = self.inner.get_mut(domain) {
                    breaker.succeed();
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut breaker = self.inner.entry(domain.to_owned()).or_default();
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
    last_attempt: DateTime<Utc>,
    last_success: DateTime<Utc>,
}

impl Breaker {
    const fn failure_threshold() -> usize {
        10
    }

    fn failure_wait() -> chrono::Duration {
        chrono::Duration::days(1)
    }

    fn should_try(&self) -> bool {
        self.failures < Self::failure_threshold()
            || self.last_attempt + Self::failure_wait() < Utc::now()
    }

    fn fail(&mut self) {
        self.failures += 1;
        self.last_attempt = Utc::now();
    }

    fn succeed(&mut self) {
        self.failures = 0;
        self.last_attempt = Utc::now();
        self.last_success = Utc::now();
    }
}

impl Default for Breaker {
    fn default() -> Self {
        let now = Utc::now();

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

impl Requests {
    pub(crate) fn new(
        key_id: String,
        private_key: RsaPrivateKey,
        user_agent: String,
        breakers: Breakers,
    ) -> Self {
        Requests {
            client: Rc::new(RefCell::new(
                Client::builder()
                    .header("User-Agent", user_agent.clone())
                    .finish(),
            )),
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
            warn!("{} consecutive errors, rebuilding http client", count);
            *self.client.borrow_mut() = Client::builder()
                .header("User-Agent", self.user_agent.clone())
                .finish();
            self.reset_err();
        }
    }

    fn reset_err(&self) {
        self.consecutive_errors.swap(0, Ordering::Relaxed);
    }

    #[tracing::instrument(name = "Fetch Json")]
    pub(crate) async fn fetch_json<T>(&self, url: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/json").await
    }

    #[tracing::instrument(name = "Fetch Activity+Json")]
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
        let parsed_url = url.parse::<Url>()?;

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
            .propagate()
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
                        debug!("Response from {}, {}", url, s);
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

    #[tracing::instrument(name = "Fetch Bytes")]
    pub(crate) async fn fetch_bytes(&self, url: &str) -> Result<(String, Bytes), Error> {
        let parsed_url = url.parse::<Url>()?;

        if !self.breakers.should_try(&parsed_url) {
            return Err(ErrorKind::Breaker.into());
        }

        info!("Fetching bytes for {}", url);
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
            .propagate()
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
                        debug!("Response from {}, {}", url, s);
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
        fields(self, inbox = inbox.to_string().as_str(), item)
    )]
    pub(crate) async fn deliver<T>(&self, inbox: Url, item: &T) -> Result<(), Error>
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

        let res = req.propagate().send_body(body).await;

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
                        debug!("Response from {}, {}", inbox.as_str(), s);
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
        let hashed = Sha256::digest(signing_string.as_bytes());
        let bytes = self.private_key.sign(
            PaddingScheme::PKCS1v15Sign {
                hash: Some(Hash::SHA2_256),
            },
            &hashed,
        )?;
        Ok(base64::encode(bytes))
    }
}
