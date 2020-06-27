use crate::{
    error::MyError,
    jobs::{Deliver, JobState},
};
use activitystreams_new::url::Url;
use anyhow::Error;
use background_jobs::ActixJob;
use futures::future::{ready, Ready};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct DeliverMany {
    to: Vec<Url>,
    data: serde_json::Value,
}

impl DeliverMany {
    pub fn new<T>(to: Vec<Url>, data: T) -> Result<Self, MyError>
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

impl ActixJob for DeliverMany {
    type State = JobState;
    type Future = Ready<Result<(), Error>>;

    const NAME: &'static str = "relay::jobs::DeliverMany";

    fn run(self, state: Self::State) -> Self::Future {
        ready(self.perform(state))
    }
}
