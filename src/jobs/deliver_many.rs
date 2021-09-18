use crate::{
    error::Error,
    jobs::{Deliver, JobState},
};
use activitystreams::url::Url;
use background_jobs::ActixJob;
use std::future::{ready, Ready};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct DeliverMany {
    to: Vec<Url>,
    data: serde_json::Value,
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
    type Future = Ready<Result<(), anyhow::Error>>;

    const NAME: &'static str = "relay::jobs::DeliverMany";

    fn run(self, state: Self::State) -> Self::Future {
        ready(self.perform(state).map_err(Into::into))
    }
}
