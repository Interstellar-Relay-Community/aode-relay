use crate::{
    apub::AcceptedObjects,
    data::Actor,
    jobs::{apub::get_inboxes, DeliverMany, JobState},
};
use background_jobs::{ActixJob, Processor};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Forward {
    input: AcceptedObjects,
    actor: Actor,
}

#[derive(Clone, Debug)]
pub struct ForwardProcessor;

impl Forward {
    pub fn new(input: AcceptedObjects, actor: Actor) -> Self {
        Forward { input, actor }
    }

    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        let object_id = self.input.object.id();

        let inboxes = get_inboxes(&state.state, &self.actor, object_id).await?;

        state
            .job_server
            .queue(DeliverMany::new(inboxes, self.input)?)?;

        Ok(())
    }
}

impl ActixJob for Forward {
    type Processor = ForwardProcessor;
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

impl Processor for ForwardProcessor {
    type Job = Forward;

    const NAME: &'static str = "ForwardProcessor";
    const QUEUE: &'static str = "default";
}
