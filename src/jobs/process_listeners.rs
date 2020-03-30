use crate::jobs::{instance::QueryInstance, nodeinfo::QueryNodeinfo, JobState};
use anyhow::Error;
use background_jobs::{ActixJob, Processor};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Listeners;

#[derive(Clone, Debug)]
pub struct ListenersProcessor;

impl Listeners {
    async fn perform(self, state: JobState) -> Result<(), Error> {
        for listener in state.state.listeners().await {
            state
                .job_server
                .queue(QueryInstance::new(listener.clone()))?;
            state.job_server.queue(QueryNodeinfo::new(listener))?;
        }

        Ok(())
    }
}

impl ActixJob for Listeners {
    type State = JobState;
    type Processor = ListenersProcessor;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

impl Processor for ListenersProcessor {
    type Job = Listeners;

    const NAME: &'static str = "ProcessListenersProcessor";
    const QUEUE: &'static str = "default";
}
