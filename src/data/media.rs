use crate::{db::Db, error::MyError};
use activitystreams::primitives::XsdAnyUri;
use bytes::Bytes;
use futures::join;
use lru::LruCache;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock};
use ttl_cache::TtlCache;
use uuid::Uuid;

static MEDIA_DURATION: Duration = Duration::from_secs(60 * 60 * 24 * 2);

#[derive(Clone)]
pub struct Media {
    db: Db,
    inverse: Arc<Mutex<HashMap<XsdAnyUri, Uuid>>>,
    url_cache: Arc<Mutex<LruCache<Uuid, XsdAnyUri>>>,
    byte_cache: Arc<RwLock<TtlCache<Uuid, (String, Bytes)>>>,
}

impl Media {
    pub fn new(db: Db) -> Self {
        Media {
            db,
            inverse: Arc::new(Mutex::new(HashMap::new())),
            url_cache: Arc::new(Mutex::new(LruCache::new(128))),
            byte_cache: Arc::new(RwLock::new(TtlCache::new(128))),
        }
    }

    pub async fn get_uuid(&self, url: &XsdAnyUri) -> Result<Option<Uuid>, MyError> {
        let res = self.inverse.lock().await.get(url).cloned();
        let uuid = match res {
            Some(uuid) => uuid,
            _ => {
                let row_opt = self
                    .db
                    .pool()
                    .get()
                    .await?
                    .query_opt(
                        "SELECT media_id
                             FROM media
                             WHERE url = $1::TEXT
                             LIMIT 1;",
                        &[&url.as_str()],
                    )
                    .await?;

                if let Some(row) = row_opt {
                    let uuid: Uuid = row.try_get(0)?;
                    self.inverse.lock().await.insert(url.clone(), uuid);
                    uuid
                } else {
                    return Ok(None);
                }
            }
        };

        if self.url_cache.lock().await.contains(&uuid) {
            return Ok(Some(uuid));
        }

        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT id
                 FROM media
                 WHERE
                    url = $1::TEXT
                 AND
                    media_id = $2::UUID
                 LIMIT 1;",
                &[&url.as_str(), &uuid],
            )
            .await?;

        if row_opt.is_some() {
            self.url_cache.lock().await.put(uuid, url.clone());

            return Ok(Some(uuid));
        }

        self.inverse.lock().await.remove(url);

        Ok(None)
    }

    pub async fn get_url(&self, uuid: Uuid) -> Result<Option<XsdAnyUri>, MyError> {
        if let Some(url) = self.url_cache.lock().await.get(&uuid).cloned() {
            return Ok(Some(url));
        }

        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT url
                 FROM media
                 WHERE media_id = $1::UUID
                 LIMIT 1;",
                &[&uuid],
            )
            .await?;

        if let Some(row) = row_opt {
            let url: String = row.try_get(0)?;
            let url: XsdAnyUri = url.parse()?;
            return Ok(Some(url));
        }

        Ok(None)
    }

    pub async fn get_bytes(&self, uuid: Uuid) -> Option<(String, Bytes)> {
        self.byte_cache.read().await.get(&uuid).cloned()
    }

    pub async fn store_url(&self, url: &XsdAnyUri) -> Result<Uuid, MyError> {
        let uuid = Uuid::new_v4();

        let (_, _, res) = join!(
            async {
                self.inverse.lock().await.insert(url.clone(), uuid);
            },
            async {
                self.url_cache.lock().await.put(uuid, url.clone());
            },
            async {
                self.db
                    .pool()
                    .get()
                    .await?
                    .execute(
                        "INSERT INTO media (
                        media_id,
                        url,
                        created_at,
                        updated_at
                     ) VALUES (
                        $1::UUID,
                        $2::TEXT,
                        'now',
                        'now'
                     ) ON CONFLICT (media_id)
                     DO UPDATE SET url = $2::TEXT;",
                        &[&uuid, &url.as_str()],
                    )
                    .await?;
                Ok(()) as Result<(), MyError>
            }
        );

        res?;

        Ok(uuid)
    }

    pub async fn store_bytes(&self, uuid: Uuid, content_type: String, bytes: Bytes) {
        self.byte_cache
            .write()
            .await
            .insert(uuid, (content_type, bytes), MEDIA_DURATION);
    }
}
