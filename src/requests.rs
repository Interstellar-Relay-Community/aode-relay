use crate::error::MyError;
use activitystreams::primitives::XsdAnyUri;
use actix_web::client::Client;
use bytes::Bytes;
use http_signature_normalization_actix::prelude::*;
use log::{error, info};
use rsa::{hash::Hashes, padding::PaddingScheme, RSAPrivateKey};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct Requests {
    client: Client,
    key_id: String,
    private_key: RSAPrivateKey,
    config: Config,
    user_agent: String,
}

impl Requests {
    pub fn new(key_id: String, private_key: RSAPrivateKey, user_agent: String) -> Self {
        Requests {
            client: Client::default(),
            key_id,
            private_key,
            config: Config::default().dont_use_created_field(),
            user_agent,
        }
    }

    pub async fn fetch<T>(&self, url: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
        let signer = self.signer();

        let mut res = self
            .client
            .get(url)
            .header("Accept", "application/activity+json")
            .header("User-Agent", self.user_agent.as_str())
            .signature(
                self.config.clone(),
                self.key_id.clone(),
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .send()
            .await
            .map_err(|e| {
                error!("Couldn't send request to {}, {}", url, e);
                MyError::SendRequest
            })?;

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        error!("Response from {}, {}", url, s);
                    }
                }
            }

            return Err(MyError::Status(res.status()));
        }

        res.json().await.map_err(|e| {
            error!("Coudn't fetch json from {}, {}", url, e);
            MyError::ReceiveResponse
        })
    }

    pub async fn fetch_bytes(&self, url: &str) -> Result<(String, Bytes), MyError> {
        info!("Fetching bytes for {}", url);
        let signer = self.signer();

        let mut res = self
            .client
            .get(url)
            .header("Accept", "application/activity+json")
            .header("User-Agent", self.user_agent.as_str())
            .signature(
                self.config.clone(),
                self.key_id.clone(),
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .send()
            .await
            .map_err(|e| {
                error!("Couldn't send request to {}, {}", url, e);
                MyError::SendRequest
            })?;

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
                        error!("Response from {}, {}", url, s);
                    }
                }
            }

            return Err(MyError::Status(res.status()));
        }

        let bytes = match res.body().limit(1024 * 1024 * 4).await {
            Err(e) => {
                error!("Coudn't fetch json from {}, {}", url, e);
                return Err(MyError::ReceiveResponse);
            }
            Ok(bytes) => bytes,
        };

        Ok((content_type, bytes))
    }

    pub async fn deliver<T>(&self, inbox: XsdAnyUri, item: &T) -> Result<(), MyError>
    where
        T: serde::ser::Serialize,
    {
        let signer = self.signer();
        let item_string = serde_json::to_string(item)?;

        let mut res = self
            .client
            .post(inbox.as_str())
            .header("Accept", "application/activity+json")
            .header("Content-Type", "application/activity+json")
            .header("User-Agent", self.user_agent.as_str())
            .signature_with_digest(
                self.config.clone(),
                self.key_id.clone(),
                Sha256::new(),
                item_string,
                move |signing_string| signer.sign(signing_string),
            )
            .await?
            .send()
            .await
            .map_err(|e| {
                error!("Couldn't send deliver request to {}, {}", inbox, e);
                MyError::SendRequest
            })?;

        if !res.status().is_success() {
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    if !s.is_empty() {
                        error!("Response from {}, {}", inbox.as_str(), s);
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
