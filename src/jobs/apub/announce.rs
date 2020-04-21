use crate::{
    config::{Config, UrlKind},
    data::Actor,
    error::MyError,
    jobs::{
        apub::{get_inboxes, prepare_activity},
        DeliverMany, JobState,
    },
};
use activitystreams::primitives::XsdAnyUri;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Announce {
    object_id: XsdAnyUri,
    actor: Actor,
}

impl Announce {
    pub fn new(object_id: XsdAnyUri, actor: Actor) -> Self {
        Announce { object_id, actor }
    }

    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        let activity_id: XsdAnyUri = state.config.generate_url(UrlKind::Activity).parse()?;

        let announce = generate_announce(&state.config, &activity_id, &self.object_id)?;
        let inboxes = get_inboxes(&state.state, &self.actor, &self.object_id).await?;
        state
            .job_server
            .queue(DeliverMany::new(inboxes, announce)?)?;

        state.state.cache(self.object_id, activity_id).await;
        Ok(())
    }
}

// Generate a type that says "Look at this object"
fn generate_announce(
    config: &Config,
    activity_id: &XsdAnyUri,
    object_id: &XsdAnyUri,
) -> Result<activitystreams::activity::Announce, MyError> {
    let mut announce = activitystreams::activity::Announce::default();

    announce
        .announce_props
        .set_object_xsd_any_uri(object_id.clone())?
        .set_actor_xsd_any_uri(config.generate_url(UrlKind::Actor))?;

    prepare_activity(
        announce,
        activity_id.clone(),
        config.generate_url(UrlKind::Followers),
    )
}

impl ActixJob for Announce {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::apub::Announce";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
