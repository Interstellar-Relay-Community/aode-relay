use crate::error::MyError;
use activitystreams_new::url::Url;
use actix_web::{client::Client, http::header::Date};
use bytes::Bytes;
use http_signature_normalization_actix::prelude::*;
use log::{debug, info, warn};
use rsa::{hash::Hashes, padding::PaddingScheme, RSAPrivateKey};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    time::SystemTime,
};

#[derive(Clone)]
pub struct Requests {
    client: Rc<RefCell<Client>>,
    consecutive_errors: Rc<AtomicUsize>,
    error_limit: usize,
    key_id: String,
    user_agent: String,
    private_key: RSAPrivateKey,
    config: Config,
}

impl Requests {
    pub fn new(key_id: String, private_key: RSAPrivateKey, user_agent: String) -> Self {
        Requests {
            client: Rc::new(RefCell::new(
                Client::build()
                    .header("User-Agent", user_agent.clone())
                    .finish(),
            )),
            consecutive_errors: Rc::new(AtomicUsize::new(0)),
            error_limit: 3,
            key_id,
            user_agent,
            private_key,
            config: Config::default().dont_use_created_field(),
        }
    }

    fn count_err(&self) {
        let count = self.consecutive_errors.fetch_add(1, Ordering::Relaxed);
        if count + 1 >= self.error_limit {
            warn!("{} consecutive errors, rebuilding http client", count);
            *self.client.borrow_mut() = Client::build()
                .header("User-Agent", self.user_agent.clone())
                .finish();
            self.reset_err();
        }
    }

    fn reset_err(&self) {
        self.consecutive_errors.swap(0, Ordering::Relaxed);
    }

    pub async fn fetch_json<T>(&self, url: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/json").await
    }

    pub async fn fetch<T>(&self, url: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/activity+json").await
    }

    async fn do_fetch<T>(&self, url: &str, accept: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
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

            return Err(MyError::Status(res.status()));
        }

        let body = res
            .body()
            .await
            .map_err(|e| MyError::ReceiveResponse(url.to_string(), e.to_string()))?;

        Ok(serde_json::from_slice(body.as_ref())?)
    }

    pub async fn fetch_bytes(&self, url: &str) -> Result<(String, Bytes), MyError> {
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

            return Err(MyError::Status(res.status()));
        }

        let bytes = match res.body().limit(1024 * 1024 * 4).await {
            Err(e) => {
                return Err(MyError::ReceiveResponse(url.to_string(), e.to_string()));
            }
            Ok(bytes) => bytes,
        };

        Ok((content_type, bytes))
    }

    pub async fn deliver<T>(&self, inbox: Url, item: &T) -> Result<(), MyError>
    where
        T: serde::ser::Serialize,
    {
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
            return Err(MyError::Status(res.status()));
        }

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
        let bytes =
            self.private_key
                .sign(PaddingScheme::PKCS1v15, Some(&Hashes::SHA2_256), &hashed)?;
        Ok(base64::encode(bytes))
    }
}
