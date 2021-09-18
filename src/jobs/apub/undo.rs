use crate::{
    apub::AcceptedActivities,
    config::UrlKind,
    db::Actor,
    error::Error,
    jobs::{apub::generate_undo_follow, Deliver, JobState},
};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Undo {
    input: AcceptedActivities,
    actor: Actor,
}

impl Undo {
    pub(crate) fn new(input: AcceptedActivities, actor: Actor) -> Self {
        Undo { input, actor }
    }

    #[tracing::instrument(name = "Undo")]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let was_following = state.state.db.is_connected(self.actor.id.clone()).await?;

        state.actors.remove_connection(&self.actor).await?;

        if was_following {
            let my_id = state.config.generate_url(UrlKind::Actor);
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
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
