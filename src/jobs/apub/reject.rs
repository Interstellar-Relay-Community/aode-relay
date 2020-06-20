use crate::{
    config::UrlKind,
    data::Actor,
    jobs::{apub::generate_undo_follow, Deliver, JobState},
};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Reject(pub Actor);

impl Reject {
    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        if state.actors.unfollower(&self.0).await?.is_some() {
            state
                .db
                .remove_listener(self.0.inbox.clone().into_inner())
                .await?;
        }

        let my_id = state.config.generate_url(UrlKind::Actor);
        let undo = generate_undo_follow(&state.config, &self.0.id, &my_id)?;

        state.job_server.queue(Deliver::new(self.0.inbox, undo)?)?;

        Ok(())
    }
}

impl ActixJob for Reject {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::apub::Reject";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
