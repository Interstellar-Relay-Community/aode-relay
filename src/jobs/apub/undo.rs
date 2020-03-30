use crate::{
    apub::AcceptedObjects,
    config::UrlKind,
    data::Actor,
    jobs::{apub::generate_undo_follow, Deliver, JobState},
};
use activitystreams::primitives::XsdAnyUri;
use background_jobs::{ActixJob, Processor};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Undo {
    input: AcceptedObjects,
    actor: Actor,
}

#[derive(Clone, Debug)]
pub struct UndoProcessor;

impl Undo {
    pub fn new(input: AcceptedObjects, actor: Actor) -> Self {
        Undo { input, actor }
    }

    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        let was_following = state.actors.is_following(&self.actor.id).await;

        if let Some(_) = state.actors.unfollower(&self.actor).await? {
            state.db.remove_listener(self.actor.inbox.clone()).await?;
        }

        if was_following {
            let my_id: XsdAnyUri = state.config.generate_url(UrlKind::Actor).parse()?;
            let undo = generate_undo_follow(&state.config, &self.actor.id, &my_id)?;
            state
                .job_server
                .queue(Deliver::new(self.actor.inbox, undo)?)?;
        }

        Ok(())
    }
}

impl ActixJob for Undo {
    type Processor = UndoProcessor;
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

impl Processor for UndoProcessor {
    type Job = Undo;

    const NAME: &'static str = "UndoProcessor";
    const QUEUE: &'static str = "default";
}
