use crate::{
    config::{Config, UrlKind},
    data::NodeCache,
    db::Db,
    error::Error,
    requests::{Breakers, Requests},
    spawner::Spawner,
};
use activitystreams::iri_string::types::IriString;
use actix_web::web;
use lru::LruCache;
use rand::thread_rng;
use rsa::{RsaPrivateKey, RsaPublicKey};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

use super::LastOnline;
use super::node::NodeConfig;

#[derive(Clone)]
pub struct State {
    pub(crate) public_key: RsaPublicKey,
    private_key: RsaPrivateKey,
    object_cache: Arc<RwLock<LruCache<IriString, IriString>>>,
    node_cache: NodeCache,
    pub(crate) node_config: Arc<RwLock<HashMap<String, NodeConfig>>>,
    breakers: Breakers,
    pub(crate) last_online: Arc<LastOnline>,
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

    pub(crate) fn requests(&self, config: &Config, spawner: Spawner) -> Requests {
        Requests::new(
            config.generate_url(UrlKind::MainKey).to_string(),
            self.private_key.clone(),
            config.user_agent(),
            self.breakers.clone(),
            self.last_online.clone(),
            config.client_pool_size(),
            config.client_timeout(),
            spawner,
        )
    }

    #[tracing::instrument(
        level = "debug",
        name = "Get inboxes for other domains",
        skip_all,
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

    pub(crate) async fn set_authority_cfg(&self, authority: &str, cfg: NodeConfig) {
        self.node_config.write().unwrap().insert(authority.to_string(), cfg);
    }

    pub(crate) async fn clear_authority_cfg(&self, authority: &str) {
        self.node_config.write().unwrap().remove(authority);
    }

    pub(crate) async fn get_authority_cfg(&self, authority: &str) -> Option<NodeConfig> {
        self.node_config.read().unwrap().get(authority).cloned()
    }

    pub(crate) async fn get_all_authority_cfg(&self) -> HashMap<String, NodeConfig> {
        self.node_config.read().unwrap().clone()
    }

    pub(crate) fn is_cached(&self, object_id: &IriString) -> bool {
        self.object_cache.read().unwrap().contains(object_id)
    }

    pub(crate) fn cache(&self, object_id: IriString, actor_id: IriString) {
        self.object_cache.write().unwrap().put(object_id, actor_id);
    }

    pub(crate) fn is_connected(&self, iri: &IriString) -> bool {
        self.breakers.should_try(iri)
    }

    #[tracing::instrument(level = "debug", name = "Building state", skip_all)]
    pub(crate) async fn build(db: Db, node_config: HashMap<String, NodeConfig>) -> Result<Self, Error> {
        let private_key = if let Ok(Some(key)) = db.private_key().await {
            tracing::debug!("Using existing key");
            key
        } else {
            tracing::info!("Generating new keys");
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
            node_config: Arc::new(RwLock::new(node_config)),
            breakers: Breakers::default(),
            db,
            last_online: Arc::new(LastOnline::empty()),
        };

        Ok(state)
    }
}
