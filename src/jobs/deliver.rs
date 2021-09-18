use crate::{error::Error, jobs::JobState};
use activitystreams::url::Url;
use background_jobs::{ActixJob, Backoff};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Deliver {
    to: Url,
    data: serde_json::Value,
}

impl Deliver {
    pub(crate) fn new<T>(to: Url, data: T) -> Result<Self, Error>
    where
        T: serde::ser::Serialize,
    {
        Ok(Deliver {
            to,
            data: serde_json::to_value(data)?,
        })
    }

    #[tracing::instrument(name = "Deliver")]
    async fn permform(self, state: JobState) -> Result<(), Error> {
        state.requests.deliver(self.to, &self.data).await?;
        Ok(())
    }
}

impl ActixJob for Deliver {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::Deliver";
    const BACKOFF: Backoff = Backoff::Exponential(8);

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.permform(state).await.map_err(Into::into) })
    }
}
