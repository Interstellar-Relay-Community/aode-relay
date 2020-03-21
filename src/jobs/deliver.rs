use crate::{error::MyError, jobs::JobState};
use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use background_jobs::{Backoff, Job, Processor};
use std::{future::Future, pin::Pin};
use tokio::sync::oneshot;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Deliver {
    to: XsdAnyUri,
    data: serde_json::Value,
}

impl Deliver {
    pub fn new<T>(to: XsdAnyUri, data: T) -> Result<Self, MyError>
    where
        T: serde::ser::Serialize,
    {
        Ok(Deliver {
            to,
            data: serde_json::to_value(data)?,
        })
    }

    async fn perform(self, state: JobState) -> Result<(), Error> {
        state.requests.deliver(self.to, &self.data).await?;

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct DeliverProcessor;

impl Job for Deliver {
    type State = JobState;
    type Processor = DeliverProcessor;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>> + Send>>;

    fn run(self, state: Self::State) -> Self::Future {
        let (tx, rx) = oneshot::channel();

        actix::spawn(async move {
            let _ = tx.send(self.perform(state).await);
        });

        Box::pin(async move { rx.await? })
    }
}

impl Processor for DeliverProcessor {
    type Job = Deliver;

    const NAME: &'static str = "DeliverProcessor";
    const QUEUE: &'static str = "default";
    const BACKOFF_STRATEGY: Backoff = Backoff::Exponential(8);
}
