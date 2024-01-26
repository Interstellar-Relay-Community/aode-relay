use crate::{
    apub::AcceptedActivities,
    db::Actor,
    error::{Error, ErrorKind},
    future::BoxFuture,
    jobs::{apub::get_inboxes, DeliverMany, JobState},
};
use activitystreams::prelude::*;
use background_jobs::Job;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct Forward {
    input: AcceptedActivities,
    actor: Actor,
}

impl std::fmt::Debug for Forward {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Forward")
            .field("input", &self.input.id_unchecked())
            .field("actor", &self.actor.id)
            .finish()
    }
}

impl Forward {
    pub fn new(input: AcceptedActivities, actor: Actor) -> Self {
        Forward { input, actor }
    }

    #[tracing::instrument(name = "Forward", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let object_id = self
            .input
            .object_unchecked()
            .as_single_id()
            .ok_or(ErrorKind::MissingId)?;

        let authority = self.actor.id.authority_str().ok_or_else(|| {
            ErrorKind::MissingDomain
        })?;

        let inboxes = get_inboxes(&state.state, &self.actor, object_id).await?;

        state
            .job_server
            .queue(DeliverMany::new(inboxes, self.input, authority.to_owned(), false)?)
            .await?;

        Ok(())
    }
}

impl Job for Forward {
    type State = JobState;
    type Future = BoxFuture<'static, anyhow::Result<()>>;

    const NAME: &'static str = "relay::jobs::apub::Forward";
    const QUEUE: &'static str = "apub";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
