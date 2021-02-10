use crate::jobs::{instance::QueryInstance, nodeinfo::QueryNodeinfo, JobState};
use anyhow::Error;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Listeners;

impl Listeners {
    async fn perform(self, state: JobState) -> Result<(), Error> {
        for actor_id in state.state.db.connected_ids().await? {
            state
                .job_server
                .queue(QueryInstance::new(actor_id.clone()))?;
            state.job_server.queue(QueryNodeinfo::new(actor_id))?;
        }

        Ok(())
    }
}

impl ActixJob for Listeners {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    const NAME: &'static str = "relay::jobs::Listeners";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
