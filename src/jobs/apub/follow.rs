use crate::{
    apub::AcceptedActivities,
    config::{Config, UrlKind},
    db::Actor,
    error::MyError,
    jobs::{apub::prepare_activity, Deliver, JobState, QueryInstance, QueryNodeinfo},
};
use activitystreams::{
    activity::{Accept as AsAccept, Follow as AsFollow},
    prelude::*,
    url::Url,
};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Follow {
    input: AcceptedActivities,
    actor: Actor,
}

impl Follow {
    pub fn new(input: AcceptedActivities, actor: Actor) -> Self {
        Follow { input, actor }
    }

    async fn perform(self, state: JobState) -> Result<(), anyhow::Error> {
        let my_id = state.config.generate_url(UrlKind::Actor);

        // if following relay directly, not just following 'public', followback
        if self.input.object_is(&my_id)
            && !state.state.db.is_connected(self.actor.id.clone()).await?
        {
            let follow = generate_follow(&state.config, &self.actor.id, &my_id)?;
            state
                .job_server
                .queue(Deliver::new(self.actor.inbox.clone(), follow)?)?;
        }

        state.actors.add_connection(self.actor.clone()).await?;

        let accept = generate_accept_follow(
            &state.config,
            &self.actor.id,
            self.input.id_unchecked().ok_or(MyError::MissingId)?,
            &my_id,
        )?;

        state
            .job_server
            .queue(Deliver::new(self.actor.inbox, accept)?)?;

        state
            .job_server
            .queue(QueryInstance::new(self.actor.id.clone()))?;

        state.job_server.queue(QueryNodeinfo::new(self.actor.id))?;

        Ok(())
    }
}

// Generate a type that says "I want to follow you"
fn generate_follow(config: &Config, actor_id: &Url, my_id: &Url) -> Result<AsFollow, MyError> {
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
    actor_id: &Url,
    input_id: &Url,
    my_id: &Url,
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
