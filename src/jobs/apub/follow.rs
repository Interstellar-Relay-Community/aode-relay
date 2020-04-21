use crate::{
    apub::AcceptedObjects,
    config::{Config, UrlKind},
    data::Actor,
    error::MyError,
    jobs::{apub::prepare_activity, Deliver, JobState},
};
use activitystreams::primitives::XsdAnyUri;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Follow {
    is_listener: bool,
    input: AcceptedObjects,
    actor: Actor,
}

impl Follow {
    pub fn new(is_listener: bool, input: AcceptedObjects, actor: Actor) -> Self {
        Follow {
            is_listener,
            input,
            actor,
        }
    }

    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        if !self.is_listener {
            state.db.add_listener(self.actor.inbox.clone()).await?;
        }
        let my_id: XsdAnyUri = state.config.generate_url(UrlKind::Actor).parse()?;

        // if following relay directly, not just following 'public', followback
        if self.input.object.is(&my_id) && !state.actors.is_following(&self.actor.id).await {
            let follow = generate_follow(&state.config, &self.actor.id, &my_id)?;
            state
                .job_server
                .queue(Deliver::new(self.actor.inbox.clone(), follow)?)?;
        }

        state.actors.follower(&self.actor).await?;

        let accept = generate_accept_follow(&state.config, &self.actor.id, &self.input.id, &my_id)?;

        state
            .job_server
            .queue(Deliver::new(self.actor.inbox, accept)?)?;
        Ok(())
    }
}

// Generate a type that says "I want to follow you"
fn generate_follow(
    config: &Config,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<activitystreams::activity::Follow, MyError> {
    let mut follow = activitystreams::activity::Follow::default();

    follow
        .follow_props
        .set_object_xsd_any_uri(actor_id.clone())?
        .set_actor_xsd_any_uri(my_id.clone())?;

    prepare_activity(
        follow,
        config.generate_url(UrlKind::Activity),
        actor_id.clone(),
    )
}

// Generate a type that says "I accept your follow request"
fn generate_accept_follow(
    config: &Config,
    actor_id: &XsdAnyUri,
    input_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<activitystreams::activity::Accept, MyError> {
    let mut accept = activitystreams::activity::Accept::default();

    accept
        .accept_props
        .set_actor_xsd_any_uri(my_id.clone())?
        .set_object_base_box({
            let mut follow = activitystreams::activity::Follow::default();

            follow.object_props.set_id(input_id.clone())?;
            follow
                .follow_props
                .set_object_xsd_any_uri(my_id.clone())?
                .set_actor_xsd_any_uri(actor_id.clone())?;

            follow
        })?;

    prepare_activity(
        accept,
        config.generate_url(UrlKind::Activity),
        actor_id.clone(),
    )
}

impl ActixJob for Follow {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "FollowProcessor";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
