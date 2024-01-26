use crate::{
    error::Error,
    future::BoxFuture,
    jobs::{instance::QueryInstance, nodeinfo::QueryNodeinfo, JobState},
};
use background_jobs::Job;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Listeners;

impl Listeners {
    #[tracing::instrument(name = "Spawn query instances", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        for actor_id in state.state.db.connected_ids().await? {
            state
                .job_server
                .queue(QueryInstance::new(actor_id.clone()))
                .await?;
            state.job_server.queue(QueryNodeinfo::new(actor_id)).await?;
        }

        Ok(())
    }
}

impl Job for Listeners {
    type State = JobState;
    type Future = BoxFuture<'static, anyhow::Result<()>>;

    const NAME: &'static str = "relay::jobs::Listeners";
    const QUEUE: &'static str = "maintenance";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
