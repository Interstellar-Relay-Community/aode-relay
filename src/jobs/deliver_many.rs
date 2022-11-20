use crate::{
    error::Error,
    jobs::{debug_object, Deliver, JobState},
};
use activitystreams::iri_string::types::IriString;
use background_jobs::ActixJob;
use futures_util::future::LocalBoxFuture;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct DeliverMany {
    to: Vec<IriString>,
    data: serde_json::Value,
}

impl std::fmt::Debug for DeliverMany {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliverMany")
            .field("activity", &self.data["type"])
            .field("object", debug_object(&self.data))
            .finish()
    }
}

impl DeliverMany {
    pub(crate) fn new<T>(to: Vec<IriString>, data: T) -> Result<Self, Error>
    where
        T: serde::ser::Serialize,
    {
        Ok(DeliverMany {
            to,
            data: serde_json::to_value(data)?,
        })
    }

    #[tracing::instrument(name = "Deliver many", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        for inbox in self.to {
            state
                .job_server
                .queue(Deliver::new(inbox, self.data.clone())?)
                .await?;
        }

        Ok(())
    }
}

impl ActixJob for DeliverMany {
    type State = JobState;
    type Future = LocalBoxFuture<'static, Result<(), anyhow::Error>>;

    const NAME: &'static str = "relay::jobs::DeliverMany";
    const QUEUE: &'static str = "deliver";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
