use crate::{
    error::Error,
    future::BoxFuture,
    jobs::{debug_object, JobState},
    requests::BreakerStrategy,
};
use activitystreams::iri_string::types::IriString;
use background_jobs::{Backoff, Job};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct Deliver {
    to: IriString,
    data: serde_json::Value,
}

impl std::fmt::Debug for Deliver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deliver")
            .field("to", &self.to.to_string())
            .field("activity", &self.data["type"])
            .field("object", debug_object(&self.data))
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
        if let Err(e) = state
            .state
            .requests
            .deliver(&self.to, &self.data, BreakerStrategy::Allow401AndBelow)
            .await
        {
            if e.is_breaker() {
                tracing::debug!("Not trying due to failed breaker");
                return Ok(());
            }
            if e.is_bad_request() {
                tracing::debug!("Server didn't understand the activity");
                return Ok(());
            }
            return Err(e);
        }
        Ok(())
    }
}

impl Job for Deliver {
    type State = JobState;
    type Future = BoxFuture<'static, anyhow::Result<()>>;

    const NAME: &'static str = "relay::jobs::Deliver";
    const QUEUE: &'static str = "deliver";
    const BACKOFF: Backoff = Backoff::Exponential(8);

    //TEMPORAL PATCH
    const MAX_RETRIES: background_jobs::MaxRetries = background_jobs::MaxRetries::Count(3);

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.permform(state).await.map_err(Into::into) })
    }
}
