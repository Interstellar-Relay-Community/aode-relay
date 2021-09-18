use crate::{
    config::{Config, UrlKind},
    data::NodeCache,
    db::Db,
    error::Error,
    requests::{Breakers, Requests},
};
use activitystreams::url::Url;
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
    config: Config,
    object_cache: Arc<RwLock<LruCache<Url, Url>>>,
    node_cache: NodeCache,
    breakers: Breakers,
    pub(crate) db: Db,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("public_key", &"PublicKey")
            .field("private_key", &"[redacted]")
            .field("config", &self.config)
            .field("object_cache", &"Object Cache")
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

    pub(crate) fn requests(&self) -> Requests {
        Requests::new(
            self.config.generate_url(UrlKind::MainKey).to_string(),
            self.private_key.clone(),
            format!(
                "Actix Web 3.0.0-alpha.1 ({}/{}; +{})",
                self.config.software_name(),
                self.config.software_version(),
                self.config.generate_url(UrlKind::Index),
            ),
            self.breakers.clone(),
        )
    }

    #[tracing::instrument(name = "Get inboxes for other domains")]
    pub(crate) async fn inboxes_without(
        &self,
        existing_inbox: &Url,
        domain: &str,
    ) -> Result<Vec<Url>, Error> {
        Ok(self
            .db
            .inboxes()
            .await?
            .iter()
            .filter_map(|inbox| {
                if let Some(dom) = inbox.domain() {
                    if inbox != existing_inbox && dom != domain {
                        return Some(inbox.clone());
                    }
                }

                None
            })
            .collect())
    }

    pub(crate) async fn is_cached(&self, object_id: &Url) -> bool {
        self.object_cache.read().await.contains(object_id)
    }

    pub(crate) async fn cache(&self, object_id: Url, actor_id: Url) {
        self.object_cache.write().await.put(object_id, actor_id);
    }

    #[tracing::instrument(name = "Building state")]
    pub(crate) async fn build(config: Config, db: Db) -> Result<Self, Error> {
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
            config,
            object_cache: Arc::new(RwLock::new(LruCache::new(1024 * 8))),
            node_cache: NodeCache::new(db.clone()),
            breakers: Breakers::default(),
            db,
        };

        Ok(state)
    }
}
