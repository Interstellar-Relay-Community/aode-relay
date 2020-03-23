use crate::jobs::{instance::QueryInstance, nodeinfo::QueryNodeinfo, JobState};
use anyhow::Error;
use background_jobs::{Job, Processor};
use std::{future::Future, pin::Pin};
use tokio::sync::oneshot;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Listeners;

#[derive(Clone, Debug)]
pub struct ListenersProcessor;

impl Listeners {
    async fn perform(self, state: JobState) -> Result<(), Error> {
        for listener in state.state.listeners().await {
            state
                .job_server
                .queue_local(QueryInstance::new(listener.clone()))?;
            state.job_server.queue_local(QueryNodeinfo::new(listener))?;
        }

        Ok(())
    }
}

impl Job for Listeners {
    type State = JobState;
    type Processor = ListenersProcessor;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>> + Send>>;

    fn run(self, state: Self::State) -> Self::Future {
        let (tx, rx) = oneshot::channel();

        actix::spawn(async move {
            let _ = tx.send(self.perform(state).await);
        });

        Box::pin(async move { rx.await? })
    }
}

impl Processor for ListenersProcessor {
    type Job = Listeners;

    const NAME: &'static str = "ProcessListenersProcessor";
    const QUEUE: &'static str = "default";
}
