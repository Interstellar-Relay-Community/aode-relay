use crate::{error::Error, jobs::JobState};
use activitystreams::iri_string::types::IriString;
use background_jobs::{ActixJob, Backoff};
use std::{future::Future, pin::Pin};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct Deliver {
    to: IriString,
    data: serde_json::Value,
}

impl std::fmt::Debug for Deliver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deliver")
            .field("to", &self.to.to_string())
            .field("data", &self.data)
            .finish()
    }
}

impl Deliver {
    pub(crate) fn new<T>(to: IriString, data: T) -> Result<Self, Error>
    where
        T: serde::ser::Serialize,
    {
        Ok(Deliver {
            to,
            data: serde_json::to_value(data)?,
        })
    }

    #[tracing::instrument(name = "Deliver", skip(state))]
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
