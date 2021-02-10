use crate::{
    config::{Config, UrlKind},
    data::NodeCache,
    db::Db,
    error::MyError,
    requests::{Breakers, Requests},
};
use activitystreams::url::Url;
use actix_web::web;
use async_rwlock::RwLock;
use log::info;
use lru::LruCache;
use rand::thread_rng;
use rsa::{RSAPrivateKey, RSAPublicKey};
use std::sync::Arc;

#[derive(Clone)]
pub struct State {
    pub(crate) public_key: RSAPublicKey,
    private_key: RSAPrivateKey,
    config: Config,
    object_cache: Arc<RwLock<LruCache<Url, Url>>>,
    node_cache: NodeCache,
    breakers: Breakers,
    pub(crate) db: Db,
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

    pub(crate) async fn inboxes_without(
        &self,
        existing_inbox: &Url,
        domain: &str,
    ) -> Result<Vec<Url>, MyError> {
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

    pub(crate) async fn build(config: Config, db: Db) -> Result<Self, MyError> {
        let private_key = if let Ok(Some(key)) = db.private_key().await {
            key
        } else {
            info!("Generating new keys");
            let key = web::block(move || {
                let mut rng = thread_rng();
                RSAPrivateKey::new(&mut rng, 4096)
            })
            .await?;

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
