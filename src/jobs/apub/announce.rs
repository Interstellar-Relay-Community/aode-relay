use crate::{
    config::{Config, UrlKind},
    db::Actor,
    error::{Error, ErrorKind},
    future::BoxFuture,
    jobs::{
        apub::{get_inboxes, prepare_activity},
        DeliverMany, JobState,
    },
};
use activitystreams::{activity::Announce as AsAnnounce, iri_string::types::IriString};
use background_jobs::Job;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct Announce {
    object_id: IriString,
    actor: Actor,
}

impl std::fmt::Debug for Announce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Announce")
            .field("object_id", &self.object_id.to_string())
            .field("actor_id", &self.actor.id)
            .finish()
    }
}

impl Announce {
    pub fn new(object_id: IriString, actor: Actor) -> Self {
        Announce { object_id, actor }
    }

    #[tracing::instrument(name = "Announce", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let activity_id = state.config.generate_url(UrlKind::Activity);

        let authority = self.actor.id.authority_str().ok_or_else(|| {
            ErrorKind::MissingDomain
        })?;

        if let Ok(node_config) = state.state.node_config.read() {
            tracing::info!("Checking if {} is receive-only", authority);
            if let Some(cfg) = node_config.get(authority) {
                if cfg.receive_only {
                    tracing::info!("{} is receive-only, skipping", authority);
                    return Ok(())
                }
            }
        } else {
            tracing::warn!("Failed to read node config, skipping receive-only check");
        }

        let announce = generate_announce(&state.config, &activity_id, &self.object_id)?;
        let inboxes = get_inboxes(&state.state, &self.actor, &self.object_id).await?;
        state
            .job_server
            .queue(DeliverMany::new(inboxes, announce, authority.to_owned(), true)?)
            .await?;

        state.state.cache(self.object_id, activity_id);
        Ok(())
    }
}

// Generate a type that says "Look at this object"
fn generate_announce(
    config: &Config,
    activity_id: &IriString,
    object_id: &IriString,
) -> Result<AsAnnounce, Error> {
    let announce = AsAnnounce::new(config.generate_url(UrlKind::Actor), object_id.clone());

    prepare_activity(
        announce,
        activity_id.clone(),
        config.generate_url(UrlKind::Followers),
    )
}

impl Job for Announce {
    type State = JobState;
    type Future = BoxFuture<'static, anyhow::Result<()>>;

    const NAME: &'static str = "relay::jobs::apub::Announce";
    const QUEUE: &'static str = "apub";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
