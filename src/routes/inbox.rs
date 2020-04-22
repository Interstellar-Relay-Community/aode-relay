use crate::{
    apub::{AcceptedObjects, ValidTypes},
    config::{Config, UrlKind},
    data::{Actor, ActorCache, State},
    error::MyError,
    jobs::apub::{Announce, Follow, Forward, Reject, Undo},
    jobs::JobServer,
    requests::Requests,
    routes::accepted,
};
use activitystreams::{primitives::XsdAnyUri, public};
use actix_web::{web, HttpResponse};
use futures::join;
use http_signature_normalization_actix::prelude::{DigestVerified, SignatureVerified};
use log::error;

pub async fn route(
    state: web::Data<State>,
    actors: web::Data<ActorCache>,
    config: web::Data<Config>,
    client: web::Data<Requests>,
    jobs: web::Data<JobServer>,
    input: web::Json<AcceptedObjects>,
    verified: Option<SignatureVerified>,
    digest_verified: Option<DigestVerified>,
) -> Result<HttpResponse, MyError> {
    let input = input.into_inner();

    let actor = actors.get(&input.actor, &client).await?.into_inner();

    let (is_blocked, is_whitelisted, is_listener) = join!(
        state.is_blocked(&actor.id),
        state.is_whitelisted(&actor.id),
        state.is_listener(&actor.inbox)
    );

    if is_blocked {
        return Err(MyError::Blocked(actor.id.to_string()));
    }

    if !is_whitelisted {
        return Err(MyError::Whitelist(actor.id.to_string()));
    }

    if !is_listener && !valid_without_listener(&input) {
        return Err(MyError::NotSubscribed(actor.inbox.to_string()));
    }

    if config.validate_signatures() && (digest_verified.is_none() || verified.is_none()) {
        return Err(MyError::NoSignature(actor.public_key_id.to_string()));
    } else if config.validate_signatures() {
        if let Some(verified) = verified {
            if actor.public_key_id.as_str() != verified.key_id() {
                error!("Bad actor, more info: {:?}", input);
                return Err(MyError::BadActor(
                    actor.public_key_id.to_string(),
                    verified.key_id().to_owned(),
                ));
            }
        }
    }

    match input.kind {
        ValidTypes::Accept => handle_accept(&config, input).await?,
        ValidTypes::Reject => handle_reject(&config, &jobs, input, actor).await?,
        ValidTypes::Announce | ValidTypes::Create => {
            handle_announce(&state, &jobs, input, actor).await?
        }
        ValidTypes::Follow => handle_follow(&config, &jobs, input, actor, is_listener).await?,
        ValidTypes::Delete | ValidTypes::Update => handle_forward(&jobs, input, actor).await?,
        ValidTypes::Undo => handle_undo(&config, &jobs, input, actor, is_listener).await?,
    };

    Ok(accepted(serde_json::json!({})))
}

fn valid_without_listener(input: &AcceptedObjects) -> bool {
    match input.kind {
        ValidTypes::Follow => true,
        ValidTypes::Undo if input.object.is_kind("Follow") => true,
        _ => false,
    }
}

async fn handle_accept(config: &Config, input: AcceptedObjects) -> Result<(), MyError> {
    if !input.object.is_kind("Follow") {
        return Err(MyError::Kind(
            input.object.kind().unwrap_or("unknown").to_owned(),
        ));
    }

    if !input
        .object
        .child_actor_is(&config.generate_url(UrlKind::Actor).parse()?)
    {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    Ok(())
}

async fn handle_reject(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: Actor,
) -> Result<(), MyError> {
    if !input.object.is_kind("Follow") {
        return Err(MyError::Kind(
            input.object.kind().unwrap_or("unknown").to_owned(),
        ));
    }

    if !input
        .object
        .child_actor_is(&config.generate_url(UrlKind::Actor).parse()?)
    {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    jobs.queue(Reject(actor))?;

    Ok(())
}

async fn handle_undo(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: Actor,
    is_listener: bool,
) -> Result<(), MyError> {
    match input.object.kind() {
        Some("Follow") | Some("Announce") | Some("Create") => (),
        _ => {
            return Err(MyError::Kind(
                input.object.kind().unwrap_or("unknown").to_owned(),
            ));
        }
    }

    if !input.object.is_kind("Follow") {
        if is_listener {
            jobs.queue(Forward::new(input, actor))?;
            return Ok(());
        } else {
            return Err(MyError::Kind(
                input.object.kind().unwrap_or("unknown").to_owned(),
            ));
        }
    }

    let my_id: XsdAnyUri = config.generate_url(UrlKind::Actor).parse()?;

    if !input.object.child_object_is(&my_id) && !input.object.child_object_is(&public()) {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    if !is_listener {
        return Ok(());
    }

    jobs.queue(Undo::new(input, actor))?;
    Ok(())
}

async fn handle_forward(
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: Actor,
) -> Result<(), MyError> {
    jobs.queue(Forward::new(input, actor))?;

    Ok(())
}

async fn handle_announce(
    state: &State,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: Actor,
) -> Result<(), MyError> {
    let object_id = input.object.id();

    if state.is_cached(object_id).await {
        return Err(MyError::Duplicate);
    }

    jobs.queue(Announce::new(object_id.to_owned(), actor))?;

    Ok(())
}

async fn handle_follow(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: Actor,
    is_listener: bool,
) -> Result<(), MyError> {
    let my_id: XsdAnyUri = config.generate_url(UrlKind::Actor).parse()?;

    if !input.object.is(&my_id) && !input.object.is(&public()) {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    jobs.queue(Follow::new(is_listener, input, actor))?;

    Ok(())
}
