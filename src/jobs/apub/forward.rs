use crate::{
    apub::AcceptedActivities,
    data::Actor,
    error::MyError,
    jobs::{apub::get_inboxes, DeliverMany, JobState},
};
use activitystreams_new::prelude::*;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Forward {
    input: AcceptedActivities,
    actor: Actor,
}

impl Forward {
    pub fn new(input: AcceptedActivities, actor: Actor) -> Self {
        Forward { input, actor }
    }

    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        let object_id = self
            .input
            .object()
            .as_single_id()
            .ok_or(MyError::MissingId)?;

        let inboxes = get_inboxes(&state.state, &self.actor, object_id).await?;

        state
            .job_server
            .queue(DeliverMany::new(inboxes, self.input)?)?;

        Ok(())
    }
}

impl ActixJob for Forward {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::apub::Forward";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
