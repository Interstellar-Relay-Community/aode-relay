use crate::{
    config::{Config, UrlKind},
    db::Actor,
    error::Error,
    jobs::{
        apub::{get_inboxes, prepare_activity},
        DeliverMany, JobState,
    },
};
use activitystreams::{activity::Announce as AsAnnounce, url::Url};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Announce {
    object_id: Url,
    actor: Actor,
}

impl Announce {
    pub fn new(object_id: Url, actor: Actor) -> Self {
        Announce { object_id, actor }
    }

    #[tracing::instrument(name = "Announce")]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let activity_id = state.config.generate_url(UrlKind::Activity);

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
    activity_id: &Url,
    object_id: &Url,
) -> Result<AsAnnounce, Error> {
    let announce = AsAnnounce::new(config.generate_url(UrlKind::Actor), object_id.clone());

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
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
