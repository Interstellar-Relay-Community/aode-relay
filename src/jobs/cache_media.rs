use crate::jobs::JobState;
use anyhow::Error;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct CacheMedia {
    uuid: Uuid,
}

impl CacheMedia {
    pub fn new(uuid: Uuid) -> Self {
        CacheMedia { uuid }
    }

    async fn perform(self, state: JobState) -> Result<(), Error> {
        if state.media.get_bytes(self.uuid).await.is_some() {
            return Ok(());
        }

        if let Some(url) = state.media.get_url(self.uuid).await? {
            let (content_type, bytes) = state.requests.fetch_bytes(url.as_str()).await?;

            state
                .media
                .store_bytes(self.uuid, content_type, bytes)
                .await;
        }

        Ok(())
    }
}

impl ActixJob for CacheMedia {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    const NAME: &'static str = "relay::jobs::CacheMedia";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
