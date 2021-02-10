use crate::{
    db::{Db, MediaMeta},
    error::MyError,
};
use activitystreams::url::Url;
use actix_web::web::Bytes;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

static MEDIA_DURATION: Duration = Duration::from_secs(60 * 60 * 24 * 2);

#[derive(Clone)]
pub struct MediaCache {
    db: Db,
}

impl MediaCache {
    pub fn new(db: Db) -> Self {
        MediaCache { db }
    }

    pub async fn get_uuid(&self, url: Url) -> Result<Option<Uuid>, MyError> {
        self.db.media_id(url).await
    }

    pub async fn get_url(&self, uuid: Uuid) -> Result<Option<Url>, MyError> {
        self.db.media_url(uuid).await
    }

    pub async fn is_outdated(&self, uuid: Uuid) -> Result<bool, MyError> {
        if let Some(meta) = self.db.media_meta(uuid).await? {
            if meta.saved_at + MEDIA_DURATION > SystemTime::now() {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn get_bytes(&self, uuid: Uuid) -> Result<Option<(String, Bytes)>, MyError> {
        if let Some(meta) = self.db.media_meta(uuid).await? {
            if meta.saved_at + MEDIA_DURATION > SystemTime::now() {
                return self
                    .db
                    .media_bytes(uuid)
                    .await
                    .map(|opt| opt.map(|bytes| (meta.media_type, bytes)));
            }
        }

        Ok(None)
    }

    pub async fn store_url(&self, url: Url) -> Result<Uuid, MyError> {
        let uuid = Uuid::new_v4();

        self.db.save_url(url, uuid).await?;

        Ok(uuid)
    }

    pub async fn store_bytes(
        &self,
        uuid: Uuid,
        media_type: String,
        bytes: Bytes,
    ) -> Result<(), MyError> {
        self.db
            .save_bytes(
                uuid,
                MediaMeta {
                    media_type,
                    saved_at: SystemTime::now(),
                },
                bytes,
            )
            .await
    }
}
