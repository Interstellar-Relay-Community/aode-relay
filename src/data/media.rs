use activitystreams::primitives::XsdAnyUri;
use bytes::Bytes;
use lru::LruCache;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock};
use ttl_cache::TtlCache;
use uuid::Uuid;

static MEDIA_DURATION: Duration = Duration::from_secs(60 * 60 * 24 * 2);

#[derive(Clone)]
pub struct Media {
    inverse: Arc<Mutex<HashMap<XsdAnyUri, Uuid>>>,
    url_cache: Arc<Mutex<LruCache<Uuid, XsdAnyUri>>>,
    byte_cache: Arc<RwLock<TtlCache<Uuid, (String, Bytes)>>>,
}

impl Media {
    pub fn new() -> Self {
        Media {
            inverse: Arc::new(Mutex::new(HashMap::new())),
            url_cache: Arc::new(Mutex::new(LruCache::new(128))),
            byte_cache: Arc::new(RwLock::new(TtlCache::new(128))),
        }
    }

    pub async fn get_uuid(&self, url: &XsdAnyUri) -> Option<Uuid> {
        let uuid = self.inverse.lock().await.get(url).cloned()?;

        if self.url_cache.lock().await.contains(&uuid) {
            return Some(uuid);
        }

        self.inverse.lock().await.remove(url);

        None
    }

    pub async fn get_url(&self, uuid: Uuid) -> Option<XsdAnyUri> {
        self.url_cache.lock().await.get(&uuid).cloned()
    }

    pub async fn get_bytes(&self, uuid: Uuid) -> Option<(String, Bytes)> {
        self.byte_cache.read().await.get(&uuid).cloned()
    }

    pub async fn store_url(&self, url: &XsdAnyUri) -> Uuid {
        let uuid = Uuid::new_v4();
        self.inverse.lock().await.insert(url.clone(), uuid);
        self.url_cache.lock().await.put(uuid, url.clone());
        uuid
    }

    pub async fn store_bytes(&self, uuid: Uuid, content_type: String, bytes: Bytes) {
        self.byte_cache
            .write()
            .await
            .insert(uuid, (content_type, bytes), MEDIA_DURATION);
    }
}
