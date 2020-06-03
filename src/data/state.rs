use crate::{
    config::{Config, UrlKind},
    data::NodeCache,
    db::Db,
    error::MyError,
    requests::Requests,
};
use activitystreams_new::primitives::XsdAnyUri;
use actix_rt::{
    spawn,
    time::{interval_at, Instant},
};
use actix_web::web;
use futures::{join, try_join};
use log::{error, info};
use lru::LruCache;
use rand::thread_rng;
use rsa::{RSAPrivateKey, RSAPublicKey};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct State {
    pub public_key: RSAPublicKey,
    private_key: RSAPrivateKey,
    config: Config,
    actor_id_cache: Arc<RwLock<LruCache<XsdAnyUri, XsdAnyUri>>>,
    blocks: Arc<RwLock<HashSet<String>>>,
    whitelists: Arc<RwLock<HashSet<String>>>,
    listeners: Arc<RwLock<HashSet<XsdAnyUri>>>,
    node_cache: NodeCache,
}

impl State {
    pub fn node_cache(&self) -> NodeCache {
        self.node_cache.clone()
    }

    pub fn requests(&self) -> Requests {
        Requests::new(
            self.config.generate_url(UrlKind::MainKey).to_string(),
            self.private_key.clone(),
            format!(
                "Actix Web 3.0.0-alpha.1 ({}/{}; +{})",
                self.config.software_name(),
                self.config.software_version(),
                self.config.generate_url(UrlKind::Index),
            ),
        )
    }

    pub async fn bust_whitelist(&self, whitelist: &str) {
        self.whitelists.write().await.remove(whitelist);
    }

    pub async fn bust_block(&self, block: &str) {
        self.blocks.write().await.remove(block);
    }

    pub async fn bust_listener(&self, inbox: &XsdAnyUri) {
        self.listeners.write().await.remove(inbox);
    }

    pub async fn listeners(&self) -> Vec<XsdAnyUri> {
        self.listeners.read().await.iter().cloned().collect()
    }

    pub async fn blocks(&self) -> Vec<String> {
        self.blocks.read().await.iter().cloned().collect()
    }

    pub async fn listeners_without(&self, inbox: &XsdAnyUri, domain: &str) -> Vec<XsdAnyUri> {
        self.listeners
            .read()
            .await
            .iter()
            .filter_map(|listener| {
                if let Some(dom) = listener.as_url().domain() {
                    if listener != inbox && dom != domain {
                        return Some(listener.clone());
                    }
                }

                None
            })
            .collect()
    }

    pub async fn is_whitelisted(&self, actor_id: &XsdAnyUri) -> bool {
        if !self.config.whitelist_mode() {
            return true;
        }

        if let Some(domain) = actor_id.as_url().domain() {
            return self.whitelists.read().await.contains(domain);
        }

        false
    }

    pub async fn is_blocked(&self, actor_id: &XsdAnyUri) -> bool {
        if let Some(domain) = actor_id.as_url().domain() {
            return self.blocks.read().await.contains(domain);
        }

        true
    }

    pub async fn is_listener(&self, actor_id: &XsdAnyUri) -> bool {
        self.listeners.read().await.contains(actor_id)
    }

    pub async fn is_cached(&self, object_id: &XsdAnyUri) -> bool {
        self.actor_id_cache.read().await.contains(object_id)
    }

    pub async fn cache(&self, object_id: XsdAnyUri, actor_id: XsdAnyUri) {
        self.actor_id_cache.write().await.put(object_id, actor_id);
    }

    pub async fn cache_block(&self, host: String) {
        self.blocks.write().await.insert(host);
    }

    pub async fn cache_whitelist(&self, host: String) {
        self.whitelists.write().await.insert(host);
    }

    pub async fn cache_listener(&self, listener: XsdAnyUri) {
        self.listeners.write().await.insert(listener);
    }

    pub async fn rehydrate(&self, db: &Db) -> Result<(), MyError> {
        let f1 = db.hydrate_blocks();
        let f2 = db.hydrate_whitelists();
        let f3 = db.hydrate_listeners();

        let (blocks, whitelists, listeners) = try_join!(f1, f2, f3)?;

        join!(
            async move {
                *self.listeners.write().await = listeners;
            },
            async move {
                *self.whitelists.write().await = whitelists;
            },
            async move {
                *self.blocks.write().await = blocks;
            }
        );

        Ok(())
    }

    pub async fn hydrate(config: Config, db: &Db) -> Result<Self, MyError> {
        let f1 = db.hydrate_blocks();
        let f2 = db.hydrate_whitelists();
        let f3 = db.hydrate_listeners();

        let f4 = async move {
            if let Ok(Some(key)) = db.hydrate_private_key().await {
                Ok(key)
            } else {
                info!("Generating new keys");
                let key = web::block(move || {
                    let mut rng = thread_rng();
                    RSAPrivateKey::new(&mut rng, 4096)
                })
                .await?;

                db.update_private_key(&key).await?;

                Ok(key)
            }
        };

        let (blocks, whitelists, listeners, private_key) = try_join!(f1, f2, f3, f4)?;

        let public_key = private_key.to_public_key();
        let listeners = Arc::new(RwLock::new(listeners));

        let state = State {
            public_key,
            private_key,
            config,
            actor_id_cache: Arc::new(RwLock::new(LruCache::new(1024 * 8))),
            blocks: Arc::new(RwLock::new(blocks)),
            whitelists: Arc::new(RwLock::new(whitelists)),
            listeners: listeners.clone(),
            node_cache: NodeCache::new(db.clone(), listeners),
        };

        state.spawn_rehydrate(db.clone());

        Ok(state)
    }

    fn spawn_rehydrate(&self, db: Db) {
        let state = self.clone();
        spawn(async move {
            let start = Instant::now();
            let duration = Duration::from_secs(60 * 10);

            let mut interval = interval_at(start, duration);

            loop {
                interval.tick().await;

                if let Err(e) = state.rehydrate(&db).await {
                    error!("Error rehydrating, {}", e);
                }
            }
        });
    }
}
