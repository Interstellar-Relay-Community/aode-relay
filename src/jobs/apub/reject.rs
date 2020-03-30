use crate::{
    config::UrlKind,
    data::Actor,
    jobs::{apub::generate_undo_follow, Deliver, JobState},
};
use activitystreams::primitives::XsdAnyUri;
use background_jobs::{ActixJob, Processor};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Reject(pub Actor);

#[derive(Clone, Debug)]
pub struct RejectProcessor;

impl Reject {
    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        if let Some(_) = state.actors.unfollower(&self.0).await? {
            state.db.remove_listener(self.0.inbox.clone()).await?;
        }

        let my_id: XsdAnyUri = state.config.generate_url(UrlKind::Actor).parse()?;
        let undo = generate_undo_follow(&state.config, &self.0.id, &my_id)?;

        state.job_server.queue(Deliver::new(self.0.inbox, undo)?)?;

        Ok(())
    }
}

impl ActixJob for Reject {
    type Processor = RejectProcessor;
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

impl Processor for RejectProcessor {
    type Job = Reject;

    const NAME: &'static str = "RejectProcessor";
    const QUEUE: &'static str = "default";
}
