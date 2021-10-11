use crate::{
    error::Error,
    jobs::{Deliver, JobState},
};
use activitystreams::url::Url;
use background_jobs::ActixJob;
use futures_util::future::LocalBoxFuture;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct DeliverMany {
    to: Vec<Url>,
    data: serde_json::Value,
}

impl std::fmt::Debug for DeliverMany {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let to = format!(
            "[{}]",
            self.to
                .iter()
                .map(|u| u.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        f.debug_struct("DeliverMany")
            .field("to", &to)
            .field("data", &self.data)
            .finish()
    }
}

impl DeliverMany {
    pub(crate) fn new<T>(to: Vec<Url>, data: T) -> Result<Self, Error>
    where
        T: serde::ser::Serialize,
    {
        Ok(DeliverMany {
            to,
            data: serde_json::to_value(data)?,
        })
    }

    #[tracing::instrument(name = "Deliver many")]
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

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
