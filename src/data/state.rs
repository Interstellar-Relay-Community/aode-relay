use crate::{
    config::{Config, UrlKind},
    data::NodeCache,
    db::Db,
    error::Error,
    requests::{Breakers, Requests},
};
use activitystreams::iri_string::types::IriString;
use actix_web::web;
use async_rwlock::RwLock;
use lru::LruCache;
use rand::thread_rng;
use rsa::{RsaPrivateKey, RsaPublicKey};
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct State {
    pub(crate) public_key: RsaPublicKey,
    private_key: RsaPrivateKey,
    object_cache: Arc<RwLock<LruCache<IriString, IriString>>>,
    node_cache: NodeCache,
    breakers: Breakers,
    pub(crate) db: Db,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("node_cache", &self.node_cache)
            .field("breakers", &self.breakers)
            .field("db", &self.db)
            .finish()
    }
}

impl State {
    pub(crate) fn node_cache(&self) -> NodeCache {
        self.node_cache.clone()
    }

    pub(crate) fn requests(&self, config: &Config) -> Requests {
        Requests::new(
            config.generate_url(UrlKind::MainKey).to_string(),
            self.private_key.clone(),
            config.user_agent(),
            self.breakers.clone(),
        )
    }

    #[tracing::instrument(
        name = "Get inboxes for other domains",
        fields(
            existing_inbox = existing_inbox.to_string().as_str(),
            authority
        )
    )]
    pub(crate) async fn inboxes_without(
        &self,
        existing_inbox: &IriString,
        authority: &str,
    ) -> Result<Vec<IriString>, Error> {
        Ok(self
            .db
            .inboxes()
            .await?
            .iter()
            .filter_map(|inbox| {
                if let Some(authority_str) = inbox.authority_str() {
                    if inbox != existing_inbox && authority_str != authority {
                        return Some(inbox.clone());
                    }
                }

                None
            })
            .collect())
    }

    pub(crate) async fn is_cached(&self, object_id: &IriString) -> bool {
        self.object_cache.read().await.contains(object_id)
    }

    pub(crate) async fn cache(&self, object_id: IriString, actor_id: IriString) {
        self.object_cache.write().await.put(object_id, actor_id);
    }

    #[tracing::instrument(name = "Building state")]
    pub(crate) async fn build(db: Db) -> Result<Self, Error> {
        let private_key = if let Ok(Some(key)) = db.private_key().await {
            info!("Using existing key");
            key
        } else {
            info!("Generating new keys");
            let key = web::block(move || {
                let mut rng = thread_rng();
                RsaPrivateKey::new(&mut rng, 4096)
            })
            .await??;

            db.update_private_key(&key).await?;

            key
        };

        let public_key = private_key.to_public_key();

        let state = State {
            public_key,
            private_key,
            object_cache: Arc::new(RwLock::new(LruCache::new(
                (1024 * 8).try_into().expect("nonzero"),
            ))),
            node_cache: NodeCache::new(db.clone()),
            breakers: Breakers::default(),
            db,
        };

        Ok(state)
    }
}
