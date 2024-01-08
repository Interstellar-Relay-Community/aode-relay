use crate::{error::Error, future::BoxFuture, jobs::JobState};
use background_jobs::{Backoff, Job};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct RecordLastOnline;

impl RecordLastOnline {
    #[tracing::instrument(skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let nodes = state.state.last_online.take();

        state.state.db.mark_last_seen(nodes).await
    }
}

impl Job for RecordLastOnline {
    type State = JobState;
    type Future = BoxFuture<'static, anyhow::Result<()>>;

    const NAME: &'static str = "relay::jobs::RecordLastOnline";
    const QUEUE: &'static str = "maintenance";
    const BACKOFF: Backoff = Backoff::Linear(1);

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
