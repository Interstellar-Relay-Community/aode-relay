use crate::{
    apub::AcceptedActors,
    config::{Config, UrlKind},
    db::Db,
    error::MyError,
    requests::Requests,
};
use activitystreams::primitives::XsdAnyUri;
use actix_web::web;
use futures::try_join;
use log::info;
use lru::LruCache;
use rand::thread_rng;
use rsa::{RSAPrivateKey, RSAPublicKey};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::RwLock;
use ttl_cache::TtlCache;

pub type ActorCache = Arc<RwLock<TtlCache<XsdAnyUri, AcceptedActors>>>;

#[derive(Clone)]
pub struct State {
    pub public_key: RSAPublicKey,
    private_key: RSAPrivateKey,
    config: Config,
    actor_cache: ActorCache,
    actor_id_cache: Arc<RwLock<LruCache<XsdAnyUri, XsdAnyUri>>>,
    blocks: Arc<RwLock<HashSet<String>>>,
    whitelists: Arc<RwLock<HashSet<String>>>,
    listeners: Arc<RwLock<HashSet<XsdAnyUri>>>,
}

impl State {
    pub fn requests(&self) -> Requests {
        Requests::new(
            self.config.generate_url(UrlKind::MainKey),
            self.private_key.clone(),
            self.actor_cache.clone(),
            format!(
                "{} {}",
                self.config.software_name(),
                self.config.software_version()
            ),
        )
    }

    pub async fn bust_whitelist(&self, whitelist: &str) {
        let mut write_guard = self.whitelists.write().await;
        write_guard.remove(whitelist);
    }

    pub async fn bust_block(&self, block: &str) {
        let mut write_guard = self.blocks.write().await;
        write_guard.remove(block);
    }

    pub async fn bust_listener(&self, inbox: &XsdAnyUri) {
        let mut write_guard = self.listeners.write().await;
        write_guard.remove(inbox);
    }

    pub async fn listeners(&self) -> Vec<XsdAnyUri> {
        let read_guard = self.listeners.read().await;
        read_guard.iter().cloned().collect()
    }

    pub async fn listeners_without(&self, inbox: &XsdAnyUri, domain: &str) -> Vec<XsdAnyUri> {
        let read_guard = self.listeners.read().await;

        read_guard
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

        if let Some(host) = actor_id.as_url().host() {
            let read_guard = self.whitelists.read().await;
            return read_guard.contains(&host.to_string());
        }

        false
    }

    pub async fn is_blocked(&self, actor_id: &XsdAnyUri) -> bool {
        if let Some(host) = actor_id.as_url().host() {
            let read_guard = self.blocks.read().await;
            return read_guard.contains(&host.to_string());
        }

        true
    }

    pub async fn is_listener(&self, actor_id: &XsdAnyUri) -> bool {
        let read_guard = self.listeners.read().await;
        read_guard.contains(actor_id)
    }

    pub async fn is_cached(&self, object_id: &XsdAnyUri) -> bool {
        let cache = self.actor_id_cache.clone();

        let read_guard = cache.read().await;
        read_guard.contains(object_id)
    }

    pub async fn cache(&self, object_id: XsdAnyUri, actor_id: XsdAnyUri) {
        let mut write_guard = self.actor_id_cache.write().await;
        write_guard.put(object_id, actor_id);
    }

    pub async fn cache_block(&self, host: String) {
        let mut write_guard = self.blocks.write().await;
        write_guard.insert(host);
    }

    pub async fn cache_whitelist(&self, host: String) {
        let mut write_guard = self.whitelists.write().await;
        write_guard.insert(host);
    }

    pub async fn cache_listener(&self, listener: XsdAnyUri) {
        let mut write_guard = self.listeners.write().await;
        write_guard.insert(listener);
    }

    pub async fn hydrate(config: Config, db: &Db) -> Result<Self, MyError> {
        let f1 = db.hydrate_blocks();
        let f2 = db.hydrate_whitelists();
        let f3 = db.hydrate_listeners();

        let f4 = async move {
            if let Some(key) = db.hydrate_private_key().await? {
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

        Ok(State {
            public_key,
            private_key,
            config,
            actor_cache: Arc::new(RwLock::new(TtlCache::new(1024 * 8))),
            actor_id_cache: Arc::new(RwLock::new(LruCache::new(1024 * 8))),
            blocks: Arc::new(RwLock::new(blocks)),
            whitelists: Arc::new(RwLock::new(whitelists)),
            listeners: Arc::new(RwLock::new(listeners)),
        })
    }
}
