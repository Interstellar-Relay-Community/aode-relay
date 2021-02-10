use crate::error::MyError;
use activitystreams::url::Url;
use actix_web::{client::Client, http::header::Date, web::Bytes};
use async_mutex::Mutex;
use async_rwlock::RwLock;
use chrono::{DateTime, Utc};
use http_signature_normalization_actix::prelude::*;
use log::{debug, info, warn};
use rsa::{hash::Hash, padding::PaddingScheme, RSAPrivateKey};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::SystemTime,
};

#[derive(Clone)]
pub(crate) struct Breakers {
    inner: Arc<RwLock<HashMap<String, Arc<Mutex<Breaker>>>>>,
}

impl Breakers {
    async fn should_try(&self, url: &Url) -> bool {
        if let Some(domain) = url.domain() {
            if let Some(breaker) = self.inner.read().await.get(domain) {
                breaker.lock().await.should_try()
            } else {
                true
            }
        } else {
            false
        }
    }

    async fn fail(&self, url: &Url) {
        if let Some(domain) = url.domain() {
            let should_write = {
                let read = self.inner.read().await;

                if let Some(breaker) = read.get(domain) {
                    let owned_breaker = Arc::clone(&breaker);
                    drop(breaker);
                    owned_breaker.lock().await.fail();
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut hm = self.inner.write().await;
                let breaker = hm
                    .entry(domain.to_owned())
                    .or_insert(Arc::new(Mutex::new(Breaker::default())));
                breaker.lock().await.fail();
            }
        }
    }

    async fn succeed(&self, url: &Url) {
        if let Some(domain) = url.domain() {
            let should_write = {
                let read = self.inner.read().await;

                if let Some(breaker) = read.get(domain) {
                    let owned_breaker = Arc::clone(&breaker);
                    drop(breaker);
                    owned_breaker.lock().await.succeed();
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut hm = self.inner.write().await;
                let breaker = hm
                    .entry(domain.to_owned())
                    .or_insert(Arc::new(Mutex::new(Breaker::default())));
                breaker.lock().await.succeed();
            }
        }
    }
}

impl Default for Breakers {
    fn default() -> Self {
        Breakers {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

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
    private_key: RSAPrivateKey,
    config: Config,
    breakers: Breakers,
}

impl Requests {
    pub(crate) fn new(
        key_id: String,
        private_key: RSAPrivateKey,
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

    pub(crate) async fn fetch_json<T>(&self, url: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/json").await
    }

    pub(crate) async fn fetch<T>(&self, url: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/activity+json").await
    }

    async fn do_fetch<T>(&self, url: &str, accept: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
        let parsed_url = url.parse::<Url>()?;

        if !self.breakers.should_try(&parsed_url).await {
            return Err(MyError::Breaker);
        }

        let signer = self.signer();

        let client: Client = self.client.borrow().clone();
        let res = client
            .get(url)
            .header("Accept", accept)
            .set(Date(SystemTime::now().into()))
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
            self.breakers.fail(&parsed_url).await;
        }

        let mut res = res.map_err(|e| MyError::SendRequest(url.to_string(), e.to_string()))?;

        self.reset_err();

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        debug!("Response from {}, {}", url, s);
                    }
                }
            }

            self.breakers.fail(&parsed_url).await;

            return Err(MyError::Status(url.to_string(), res.status()));
        }

        self.breakers.succeed(&parsed_url).await;

        let body = res
            .body()
            .await
            .map_err(|e| MyError::ReceiveResponse(url.to_string(), e.to_string()))?;

        Ok(serde_json::from_slice(body.as_ref())?)
    }

    pub(crate) async fn fetch_bytes(&self, url: &str) -> Result<(String, Bytes), MyError> {
        let parsed_url = url.parse::<Url>()?;

        if !self.breakers.should_try(&parsed_url).await {
            return Err(MyError::Breaker);
        }

        info!("Fetching bytes for {}", url);
        let signer = self.signer();

        let client: Client = self.client.borrow().clone();
        let res = client
            .get(url)
            .header("Accept", "*/*")
            .set(Date(SystemTime::now().into()))
            .signature(
                self.config.clone(),
                self.key_id.clone(),
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .send()
            .await;

        if res.is_err() {
            self.breakers.fail(&parsed_url).await;
            self.count_err();
        }

        let mut res = res.map_err(|e| MyError::SendRequest(url.to_string(), e.to_string()))?;

        self.reset_err();

        let content_type = if let Some(content_type) = res.headers().get("content-type") {
            if let Ok(s) = content_type.to_str() {
                s.to_owned()
            } else {
                return Err(MyError::ContentType);
            }
        } else {
            return Err(MyError::ContentType);
        };

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        debug!("Response from {}, {}", url, s);
                    }
                }
            }

            self.breakers.fail(&parsed_url).await;

            return Err(MyError::Status(url.to_string(), res.status()));
        }

        self.breakers.succeed(&parsed_url).await;

        let bytes = match res.body().limit(1024 * 1024 * 4).await {
            Err(e) => {
                return Err(MyError::ReceiveResponse(url.to_string(), e.to_string()));
            }
            Ok(bytes) => bytes,
        };

        Ok((content_type, bytes))
    }

    pub(crate) async fn deliver<T>(&self, inbox: Url, item: &T) -> Result<(), MyError>
    where
        T: serde::ser::Serialize,
    {
        if !self.breakers.should_try(&inbox).await {
            return Err(MyError::Breaker);
        }

        let signer = self.signer();
        let item_string = serde_json::to_string(item)?;

        let client: Client = self.client.borrow().clone();
        let res = client
            .post(inbox.as_str())
            .header("Accept", "application/activity+json")
            .header("Content-Type", "application/activity+json")
            .set(Date(SystemTime::now().into()))
            .signature_with_digest(
                self.config.clone(),
                self.key_id.clone(),
                Sha256::new(),
                item_string,
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .send()
            .await;

        if res.is_err() {
            self.count_err();
            self.breakers.fail(&inbox).await;
        }

        let mut res = res.map_err(|e| MyError::SendRequest(inbox.to_string(), e.to_string()))?;

        self.reset_err();

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        debug!("Response from {}, {}", inbox.as_str(), s);
                    }
                }
            }

            self.breakers.fail(&inbox).await;
            return Err(MyError::Status(inbox.to_string(), res.status()));
        }

        self.breakers.succeed(&inbox).await;

        Ok(())
    }

    fn signer(&self) -> Signer {
        Signer {
            private_key: self.private_key.clone(),
        }
    }
}

struct Signer {
    private_key: RSAPrivateKey,
}

impl Signer {
    fn sign(&self, signing_string: &str) -> Result<String, MyError> {
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
