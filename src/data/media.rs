use crate::{db::Db, error::Error};
use activitystreams::iri_string::types::IriString;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct MediaCache {
    db: Db,
}

impl MediaCache {
    pub(crate) fn new(db: Db) -> Self {
        MediaCache { db }
    }

    #[tracing::instrument(level = "debug", name = "Get media uuid", skip_all, fields(url = url.to_string().as_str()))]
    pub(crate) async fn get_uuid(&self, url: IriString) -> Result<Option<Uuid>, Error> {
        self.db.media_id(url).await
    }

    #[tracing::instrument(level = "debug", name = "Get media url", skip(self))]
    pub(crate) async fn get_url(&self, uuid: Uuid) -> Result<Option<IriString>, Error> {
        self.db.media_url(uuid).await
    }

    #[tracing::instrument(name = "Store media url", skip_all, fields(url = url.to_string().as_str()))]
    pub(crate) async fn store_url(&self, url: IriString) -> Result<Uuid, Error> {
        let uuid = Uuid::new_v4();

        self.db.save_url(url, uuid).await?;

        Ok(uuid)
    }
}
