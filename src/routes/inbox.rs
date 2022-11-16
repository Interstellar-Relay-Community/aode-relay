use crate::{
    apub::{AcceptedActivities, AcceptedUndoObjects, UndoTypes, ValidTypes},
    config::{Config, UrlKind},
    data::{ActorCache, State},
    db::Actor,
    error::{Error, ErrorKind},
    jobs::apub::{Announce, Follow, Forward, Reject, Undo},
    jobs::JobServer,
    requests::Requests,
    routes::accepted,
};
use activitystreams::{
    activity, base::AnyBase, iri_string::types::IriString, prelude::*, primitives::OneOrMany,
    public,
};
use actix_web::{web, HttpResponse};
use http_signature_normalization_actix::prelude::{DigestVerified, SignatureVerified};

#[tracing::instrument(name = "Inbox", skip(actors, client, jobs, config, state, verified))]
pub(crate) async fn route(
    state: web::Data<State>,
    actors: web::Data<ActorCache>,
    config: web::Data<Config>,
    client: web::Data<Requests>,
    jobs: web::Data<JobServer>,
    input: web::Json<AcceptedActivities>,
    verified: Option<(SignatureVerified, DigestVerified)>,
) -> Result<HttpResponse, Error> {
    let input = input.into_inner();

    let actor = actors
        .get(
            input.actor()?.as_single_id().ok_or(ErrorKind::MissingId)?,
            &client,
        )
        .await?
        .into_inner();

    let is_allowed = state.db.is_allowed(actor.id.clone());
    let is_connected = state.db.is_connected(actor.id.clone());

    let (is_allowed, is_connected) = tokio::try_join!(is_allowed, is_connected)?;

    if !is_allowed {
        return Err(ErrorKind::NotAllowed(actor.id.to_string()).into());
    }

    if !is_connected && !valid_without_listener(&input)? {
        return Err(ErrorKind::NotSubscribed(actor.id.to_string()).into());
    }

    if config.validate_signatures() && verified.is_none() {
        return Err(ErrorKind::NoSignature(actor.public_key_id.to_string()).into());
    } else if config.validate_signatures() {
        if let Some((verified, _)) = verified {
            if actor.public_key_id.as_str() != verified.key_id() {
                tracing::error!("Bad actor, more info: {:?}", input);
                return Err(ErrorKind::BadActor(
                    actor.public_key_id.to_string(),
                    verified.key_id().to_owned(),
                )
                .into());
            }
        }
    }

    match input.kind().ok_or(ErrorKind::MissingKind)? {
        ValidTypes::Accept => handle_accept(&config, input).await?,
        ValidTypes::Reject => handle_reject(&config, &jobs, input, actor).await?,
        ValidTypes::Announce | ValidTypes::Create => {
            handle_announce(&state, &jobs, input, actor).await?
        }
        ValidTypes::Follow => handle_follow(&config, &jobs, input, actor).await?,
        ValidTypes::Delete | ValidTypes::Update => handle_forward(&jobs, input, actor).await?,
        ValidTypes::Undo => handle_undo(&config, &jobs, input, actor, is_connected).await?,
    };

    Ok(accepted(serde_json::json!({})))
}

fn valid_without_listener(input: &AcceptedActivities) -> Result<bool, Error> {
    match input.kind() {
        Some(ValidTypes::Follow) => Ok(true),
        Some(ValidTypes::Undo) => Ok(single_object(input.object_unchecked())?.is_kind("Follow")),
        _ => Ok(false),
    }
}

fn kind_str(base: &AnyBase) -> Result<&str, Error> {
    base.kind_str()
        .ok_or(ErrorKind::MissingKind)
        .map_err(Into::into)
}

fn id_string(id: Option<&IriString>) -> Result<String, Error> {
    id.map(|s| s.to_string())
        .ok_or(ErrorKind::MissingId)
        .map_err(Into::into)
}

fn single_object(o: &OneOrMany<AnyBase>) -> Result<&AnyBase, Error> {
    o.as_one().ok_or(ErrorKind::ObjectCount).map_err(Into::into)
}

async fn handle_accept(config: &Config, input: AcceptedActivities) -> Result<(), Error> {
    let base = single_object(input.object_unchecked())?.clone();
    let follow = if let Some(follow) = activity::Follow::from_any_base(base)? {
        follow
    } else {
        return Err(ErrorKind::Kind(
            kind_str(single_object(input.object_unchecked())?)?.to_owned(),
        )
        .into());
    };

    if !follow.actor_is(&config.generate_url(UrlKind::Actor)) {
        return Err(ErrorKind::WrongActor(id_string(follow.actor()?.as_single_id())?).into());
    }

    Ok(())
}

async fn handle_reject(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
) -> Result<(), Error> {
    let base = single_object(input.object_unchecked())?.clone();
    let follow = if let Some(follow) = activity::Follow::from_any_base(base)? {
        follow
    } else {
        return Err(ErrorKind::Kind(
            kind_str(single_object(input.object_unchecked())?)?.to_owned(),
        )
        .into());
    };

    if !follow.actor_is(&config.generate_url(UrlKind::Actor)) {
        return Err(ErrorKind::WrongActor(id_string(follow.actor()?.as_single_id())?).into());
    }

    jobs.queue(Reject(actor)).await?;

    Ok(())
}

async fn handle_undo(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
    is_listener: bool,
) -> Result<(), Error> {
    let any_base = single_object(input.object_unchecked())?.clone();
    let undone_object =
        AcceptedUndoObjects::from_any_base(any_base)?.ok_or(ErrorKind::ObjectFormat)?;

    if !undone_object.is_kind(&UndoTypes::Follow) {
        if is_listener {
            jobs.queue(Forward::new(input, actor)).await?;
            return Ok(());
        } else {
            return Err(ErrorKind::NotSubscribed(actor.id.to_string()).into());
        }
    }

    let my_id: IriString = config.generate_url(UrlKind::Actor);

    if !undone_object.object_is(&my_id) && !undone_object.object_is(&public()) {
        return Err(ErrorKind::WrongActor(id_string(
            undone_object.object_unchecked().as_single_id(),
        )?)
        .into());
    }

    if !is_listener {
        return Ok(());
    }

    jobs.queue(Undo::new(input, actor)).await?;
    Ok(())
}

async fn handle_forward(
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
) -> Result<(), Error> {
    jobs.queue(Forward::new(input, actor)).await?;

    Ok(())
}

async fn handle_announce(
    state: &State,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
) -> Result<(), Error> {
    let object_id = input
        .object_unchecked()
        .as_single_id()
        .ok_or(ErrorKind::MissingId)?;

    if state.is_cached(object_id).await {
        return Err(ErrorKind::Duplicate.into());
    }

    jobs.queue(Announce::new(object_id.to_owned(), actor))
        .await?;

    Ok(())
}

async fn handle_follow(
    config: &Config,
    jobs: &JobServer,
    input: AcceptedActivities,
    actor: Actor,
) -> Result<(), Error> {
    let my_id: IriString = config.generate_url(UrlKind::Actor);

    if !input.object_is(&my_id) && !input.object_is(&public()) {
        return Err(
            ErrorKind::WrongActor(id_string(input.object_unchecked().as_single_id())?).into(),
        );
    }

    jobs.queue(Follow::new(input, actor)).await?;

    Ok(())
}
