use crate::{
    db::{Db, MediaMeta},
    error::Error,
};
use activitystreams::iri_string::types::IriString;
use actix_web::web::Bytes;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

static MEDIA_DURATION: Duration = Duration::from_secs(60 * 60 * 24 * 2);

#[derive(Clone, Debug)]
pub struct MediaCache {
    db: Db,
}

impl MediaCache {
    pub(crate) fn new(db: Db) -> Self {
        MediaCache { db }
    }

    #[tracing::instrument(name = "Get media uuid", skip_all, fields(url = url.to_string().as_str()))]
    pub(crate) async fn get_uuid(&self, url: IriString) -> Result<Option<Uuid>, Error> {
        self.db.media_id(url).await
    }

    #[tracing::instrument(name = "Get media url", skip(self))]
    pub(crate) async fn get_url(&self, uuid: Uuid) -> Result<Option<IriString>, Error> {
        self.db.media_url(uuid).await
    }

    #[tracing::instrument(name = "Is media outdated", skip(self))]
    pub(crate) async fn is_outdated(&self, uuid: Uuid) -> Result<bool, Error> {
        if let Some(meta) = self.db.media_meta(uuid).await? {
            if meta.saved_at + MEDIA_DURATION > SystemTime::now() {
                return Ok(false);
            }
        }

        Ok(true)
    }

    #[tracing::instrument(name = "Get media bytes", skip(self))]
    pub(crate) async fn get_bytes(&self, uuid: Uuid) -> Result<Option<(String, Bytes)>, Error> {
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

    #[tracing::instrument(name = "Store media url", skip_all, fields(url = url.to_string().as_str()))]
    pub(crate) async fn store_url(&self, url: IriString) -> Result<Uuid, Error> {
        let uuid = Uuid::new_v4();

        self.db.save_url(url, uuid).await?;

        Ok(uuid)
    }

    #[tracing::instrument(name = "store media bytes", skip(self, bytes))]
    pub(crate) async fn store_bytes(
        &self,
        uuid: Uuid,
        media_type: String,
        bytes: Bytes,
    ) -> Result<(), Error> {
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
