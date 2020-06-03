use crate::{
    apub::AcceptedActivities,
    config::{Config, UrlKind},
    data::Actor,
    error::MyError,
    jobs::{apub::prepare_activity, Deliver, JobState},
};
use activitystreams_new::{
    activity::{Accept as AsAccept, Follow as AsFollow},
    prelude::*,
    primitives::XsdAnyUri,
};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Follow {
    is_listener: bool,
    input: AcceptedActivities,
    actor: Actor,
}

impl Follow {
    pub fn new(is_listener: bool, input: AcceptedActivities, actor: Actor) -> Self {
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
        let my_id = state.config.generate_url(UrlKind::Actor);

        // if following relay directly, not just following 'public', followback
        if self.input.object_is(&my_id) && !state.actors.is_following(&self.actor.id).await {
            let follow = generate_follow(&state.config, &self.actor.id, &my_id)?;
            state
                .job_server
                .queue(Deliver::new(self.actor.inbox.clone(), follow)?)?;
        }

        state.actors.follower(&self.actor).await?;

        let accept = generate_accept_follow(
            &state.config,
            &self.actor.id,
            self.input.id().ok_or(MyError::MissingId)?,
            &my_id,
        )?;

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
) -> Result<AsFollow, MyError> {
    let follow = AsFollow::new(my_id.clone(), actor_id.clone());

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
) -> Result<AsAccept, MyError> {
    let mut follow = AsFollow::new(actor_id.clone(), my_id.clone());

    follow.set_id(input_id.clone());

    let accept = AsAccept::new(my_id.clone(), follow.into_any_base()?);

    prepare_activity(
        accept,
        config.generate_url(UrlKind::Activity),
        actor_id.clone(),
    )
}

impl ActixJob for Follow {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::apub::Follow";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}
