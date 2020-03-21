use crate::{
    error::MyError,
    jobs::{Deliver, JobState},
};
use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use background_jobs::{Job, Processor};
use futures::future::{ready, Ready};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct DeliverMany {
    to: Vec<XsdAnyUri>,
    data: serde_json::Value,
}

impl DeliverMany {
    pub fn new<T>(to: Vec<XsdAnyUri>, data: T) -> Result<Self, MyError>
    where
        T: serde::ser::Serialize,
    {
        Ok(DeliverMany {
            to,
            data: serde_json::to_value(data)?,
        })
    }

    fn perform(self, state: JobState) -> Result<(), Error> {
        for inbox in self.to {
            state
                .job_server
                .queue(Deliver::new(inbox, self.data.clone())?)?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct DeliverManyProcessor;

impl Job for DeliverMany {
    type State = JobState;
    type Processor = DeliverManyProcessor;
    type Future = Ready<Result<(), Error>>;

    fn run(self, state: Self::State) -> Self::Future {
        ready(self.perform(state))
    }
}

impl Processor for DeliverManyProcessor {
    type Job = DeliverMany;

    const NAME: &'static str = "DeliverManyProcessor";
    const QUEUE: &'static str = "default";
}
