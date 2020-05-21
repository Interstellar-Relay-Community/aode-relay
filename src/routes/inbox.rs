use crate::{
    apub::{AcceptedActivities, AcceptedUndoObjects, UndoTypes, ValidTypes},
    config::{Config, UrlKind},
    data::{Actor, ActorCache, State},
    error::MyError,
    jobs::apub::{Announce, Follow, Forward, Reject, Undo},
    jobs::JobServer,
    requests::Requests,
    routes::accepted,
};
use activitystreams_new::{
    activity,
    base::AnyBase,
    prelude::*,
    primitives::{OneOrMany, XsdAnyUri},
    public,
};
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
    input: web::Json<AcceptedActivities>,
    verified: Option<(SignatureVerified, DigestVerified)>,
) -> Result<HttpResponse, MyError> {
    let input = input.into_inner();

    let actor = actors
        .get(
            input.actor().as_single_id().ok_or(MyError::MissingId)?,
            &client,
        )
        .await?
        .into_inner();

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

    if !is_listener && !valid_without_listener(&input)? {
        return Err(MyError::NotSubscribed(actor.inbox.to_string()));
    }

    if config.validate_signatures() && verified.is_none() {
        return Err(MyError::NoSignature(actor.public_key_id.to_string()));
    } else if config.validate_signatures() {
        if let Some((verified, _)) = verified {
            if actor.public_key_id.as_str() != verified.key_id() {
                error!("Bad actor, more info: {:?}", input);
                return Err(MyError::BadActor(
                    actor.public_key_id.to_string(),
                    verified.key_id().to_owned(),
                ));
            }
        }
    }

    match input.kind().ok_or(MyError::MissingKind)? {
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

fn valid_without_listener(input: &AcceptedActivities) -> Result<bool, MyError> {
    match input.kind() {
        Some(ValidTypes::Follow) => Ok(true),
        Some(ValidTypes::Undo) => Ok(single_object(input.object())?.is_kind("Follow")),
        _ => Ok(false),
    }
}

fn kind_str(base: &AnyBase) -> Result<&str, MyError> {
    base.kind_str().ok_or(MyError::MissingKind)
}

fn id_string(id: Option<&XsdAnyUri>) -> Result<String, MyError> {
    id.map(|s| s.to_string()).ok_or(MyError::MissingId)
}

fn single_object(o: &OneOrMany<AnyBase>) -> Result<&AnyBase, MyError> {
    o.as_one().ok_or(MyError::ObjectCount)
}

async fn handle_accept(config: &Config, input: AcceptedActivities) -> Result<(), MyError> {
    let follow = if let Ok(Some(follow)) =
        activity::Follow::from_any_base(single_object(input.object())?.clone())
    {
        follow
    } else {
        return Err(MyError::Kind(
            kind_str(single_object(input.object())?)?.to_owned(),
        ));
    };

    if !follow.actor_is(&config.generate_url(UrlKind::Actor).parse()?) {
        return Err(MyError::WrongActor(id_string(follow.id())?));
    }

    Ok(())
}

async fn handle_reject(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
) -> Result<(), MyError> {
    let follow = if let Ok(Some(follow)) =
        activity::Follow::from_any_base(single_object(input.object())?.clone())
    {
        follow
    } else {
        return Err(MyError::Kind(
            kind_str(single_object(input.object())?)?.to_owned(),
        ));
    };

    if !follow.actor_is(&config.generate_url(UrlKind::Actor).parse()?) {
        return Err(MyError::WrongActor(
            follow.id().map(|s| s.to_string()).unwrap_or(String::new()),
        ));
    }

    jobs.queue(Reject(actor))?;

    Ok(())
}

async fn handle_undo(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
    is_listener: bool,
) -> Result<(), MyError> {
    let any_base = single_object(input.object())?.clone();
    let undone_object =
        AcceptedUndoObjects::from_any_base(any_base)?.ok_or(MyError::ObjectFormat)?;

    if !undone_object.is_kind(&UndoTypes::Follow) {
        if is_listener {
            jobs.queue(Forward::new(input, actor))?;
            return Ok(());
        } else {
            return Err(MyError::NotSubscribed(actor.inbox.to_string()));
        }
    }

    let my_id: XsdAnyUri = config.generate_url(UrlKind::Actor).parse()?;

    if !undone_object.object_is(&my_id) && !undone_object.object_is(&public()) {
        return Err(MyError::WrongActor(id_string(undone_object.id())?));
    }

    if !is_listener {
        return Ok(());
    }

    jobs.queue(Undo::new(input, actor))?;
    Ok(())
}

async fn handle_forward(
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
) -> Result<(), MyError> {
    jobs.queue(Forward::new(input, actor))?;

    Ok(())
}

async fn handle_announce(
    state: &State,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
) -> Result<(), MyError> {
    let object_id = input.object().as_single_id().ok_or(MyError::MissingId)?;

    if state.is_cached(object_id).await {
        return Err(MyError::Duplicate);
    }

    jobs.queue(Announce::new(object_id.to_owned(), actor))?;

    Ok(())
}

async fn handle_follow(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
    is_listener: bool,
) -> Result<(), MyError> {
    let my_id: XsdAnyUri = config.generate_url(UrlKind::Actor).parse()?;

    if !input.object_is(&my_id) && !input.object_is(&public()) {
        return Err(MyError::WrongActor(id_string(
            input.object().as_single_id(),
        )?));
    }

    jobs.queue(Follow::new(is_listener, input, actor))?;

    Ok(())
}
