use crate::{
    apub::AcceptedObjects,
    config::UrlKind,
    data::Actor,
    jobs::{apub::generate_undo_follow, Deliver, JobState},
};
use activitystreams::primitives::XsdAnyUri;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Undo {
    input: AcceptedObjects,
    actor: Actor,
}

impl Undo {
    pub fn new(input: AcceptedObjects, actor: Actor) -> Self {
        Undo { input, actor }
    }

    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        let was_following = state.actors.is_following(&self.actor.id).await;

        if state.actors.unfollower(&self.actor).await?.is_some() {
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
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::apub::Undo";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
