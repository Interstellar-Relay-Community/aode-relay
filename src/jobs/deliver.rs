use crate::{error::MyError, jobs::JobState};
use activitystreams::url::Url;
use anyhow::Error;
use background_jobs::{ActixJob, Backoff};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Deliver {
    to: Url,
    data: serde_json::Value,
}

impl Deliver {
    pub fn new<T>(to: Url, data: T) -> Result<Self, MyError>
    where
        T: serde::ser::Serialize,
    {
        Ok(Deliver {
            to,
            data: serde_json::to_value(data)?,
        })
    }
}

impl ActixJob for Deliver {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    const NAME: &'static str = "relay::jobs::Deliver";
    const BACKOFF: Backoff = Backoff::Exponential(8);

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move {
            state.requests.deliver(self.to, &self.data).await?;

            Ok(())
        })
    }
}
