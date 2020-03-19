use crate::{apub::AcceptedActors, error::MyError, state::ActorCache};
use activitystreams::primitives::XsdAnyUri;
use actix::Arbiter;
use actix_web::client::Client;
use futures::stream::StreamExt;
use http_signature_normalization_actix::prelude::*;
use log::error;
use rsa::{hash::Hashes, padding::PaddingScheme, RSAPrivateKey};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct Requests {
    client: Client,
    key_id: String,
    private_key: RSAPrivateKey,
    actor_cache: ActorCache,
    config: Config,
    user_agent: String,
}

impl Requests {
    pub fn new(
        key_id: String,
        private_key: RSAPrivateKey,
        actor_cache: ActorCache,
        user_agent: String,
    ) -> Self {
        Requests {
            client: Client::default(),
            key_id,
            private_key,
            actor_cache,
            config: Config::default().dont_use_created_field(),
            user_agent,
        }
    }

    pub async fn fetch_actor(&self, actor_id: &XsdAnyUri) -> Result<AcceptedActors, MyError> {
        if let Some(actor) = self.get_actor(actor_id).await {
            return Ok(actor);
        }

        let actor: AcceptedActors = self.fetch(actor_id.as_str()).await?;

        self.cache_actor(actor_id.to_owned(), actor.clone()).await;

        Ok(actor)
    }

    pub async fn fetch<T>(&self, url: &str) -> Result<T, MyError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut res = self
            .client
            .get(url)
            .header("Accept", "application/activity+json")
            .header("User-Agent", self.user_agent.as_str())
            .signature(&self.config, &self.key_id, |signing_string| {
                self.sign(signing_string)
            })?
            .send()
            .await
            .map_err(|e| {
                error!("Couldn't send request to {}, {}", url, e);
                MyError::SendRequest
            })?;

        if !res.status().is_success() {
            error!("Invalid status code for fetch, {}", res.status());
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    error!("Response, {}", s);
                }
            }

            return Err(MyError::Status);
        }

        res.json().await.map_err(|e| {
            error!("Coudn't fetch json from {}, {}", url, e);
            MyError::ReceiveResponse
        })
    }

    pub fn deliver_many<T>(&self, inboxes: Vec<XsdAnyUri>, item: T)
    where
        T: serde::ser::Serialize + 'static,
    {
        let this = self.clone();

        Arbiter::spawn(async move {
            let mut unordered = futures::stream::FuturesUnordered::new();

            for inbox in inboxes {
                unordered.push(this.deliver(inbox, &item));
            }

            while let Some(_) = unordered.next().await {}
        });
    }

    pub async fn deliver<T>(&self, inbox: XsdAnyUri, item: &T) -> Result<(), MyError>
    where
        T: serde::ser::Serialize,
    {
        let mut digest = Sha256::new();

        let item_string = serde_json::to_string(item)?;

        let mut res = self
            .client
            .post(inbox.as_str())
            .header("Accept", "application/activity+json")
            .header("Content-Type", "application/activity+json")
            .header("User-Agent", self.user_agent.as_str())
            .signature_with_digest(
                &self.config,
                &self.key_id,
                &mut digest,
                item_string,
                |signing_string| self.sign(signing_string),
            )?
            .send()
            .await
            .map_err(|e| {
                error!("Couldn't send deliver request to {}, {}", inbox, e);
                MyError::SendRequest
            })?;

        if !res.status().is_success() {
            error!("Invalid response status from {}, {}", inbox, res.status());
            if let Ok(bytes) = res.body().await {
                if let Ok(s) = String::from_utf8(bytes.as_ref().to_vec()) {
                    error!("Response, {}", s);
                }
            }
            return Err(MyError::Status);
        }

        Ok(())
    }

    fn sign(&self, signing_string: &str) -> Result<String, crate::error::MyError> {
        let hashed = Sha256::digest(signing_string.as_bytes());
        let bytes =
            self.private_key
                .sign(PaddingScheme::PKCS1v15, Some(&Hashes::SHA2_256), &hashed)?;
        Ok(base64::encode(bytes))
    }

    async fn get_actor(&self, actor_id: &XsdAnyUri) -> Option<AcceptedActors> {
        let cache = self.actor_cache.clone();

        let read_guard = cache.read().await;
        read_guard.get(actor_id).cloned()
    }

    async fn cache_actor(&self, actor_id: XsdAnyUri, actor: AcceptedActors) {
        let cache = self.actor_cache.clone();

        let mut write_guard = cache.write().await;
        write_guard.insert(actor_id, actor, std::time::Duration::from_secs(3600));
    }
}
