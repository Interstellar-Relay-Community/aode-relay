use crate::{
    apub::{AcceptedActors, AcceptedObjects, ValidTypes},
    config::{Config, UrlKind},
    db::Db,
    error::MyError,
    jobs::JobServer,
    jobs::{Deliver, DeliverMany},
    requests::Requests,
    routes::accepted,
    state::State,
};
use activitystreams::{
    activity::{Accept, Announce, Follow, Undo},
    context,
    object::properties::ObjectProperties,
    primitives::XsdAnyUri,
    public, security,
};
use actix_web::{web, HttpResponse};
use futures::join;
use http_signature_normalization_actix::prelude::{DigestVerified, SignatureVerified};
use log::error;
use std::convert::TryInto;

pub async fn route(
    db: web::Data<Db>,
    state: web::Data<State>,
    config: web::Data<Config>,
    client: web::Data<Requests>,
    jobs: web::Data<JobServer>,
    input: web::Json<AcceptedObjects>,
    verified: Option<SignatureVerified>,
    digest_verified: Option<DigestVerified>,
) -> Result<HttpResponse, MyError> {
    let input = input.into_inner();

    let actor = client.fetch_actor(&input.actor).await?;

    let (is_blocked, is_whitelisted, is_listener) = join!(
        state.is_blocked(&actor.id),
        state.is_whitelisted(&actor.id),
        state.is_listener(actor.inbox())
    );

    if is_blocked {
        return Err(MyError::Blocked(actor.id.to_string()));
    }

    if !is_whitelisted {
        return Err(MyError::Whitelist(actor.id.to_string()));
    }

    if !is_listener && !valid_without_listener(&input) {
        return Err(MyError::NotSubscribed(actor.inbox().to_string()));
    }

    if config.validate_signatures() && (digest_verified.is_none() || verified.is_none()) {
        return Err(MyError::NoSignature(actor.public_key.id.to_string()));
    } else if config.validate_signatures() {
        if let Some(verified) = verified {
            if actor.public_key.id.as_str() != verified.key_id() {
                error!("Bad actor, more info: {:?}", input);
                return Err(MyError::BadActor(
                    actor.public_key.id.to_string(),
                    verified.key_id().to_owned(),
                ));
            }
        }
    }

    match input.kind {
        ValidTypes::Accept => handle_accept(&config, input).await,
        ValidTypes::Reject => handle_reject(&db, &config, &jobs, input, actor).await,
        ValidTypes::Announce | ValidTypes::Create => {
            handle_announce(&state, &config, &jobs, input, actor).await
        }
        ValidTypes::Follow => handle_follow(&db, &config, &jobs, input, actor, is_listener).await,
        ValidTypes::Delete | ValidTypes::Update => {
            handle_forward(&state, &jobs, input, actor).await
        }
        ValidTypes::Undo => {
            handle_undo(&db, &state, &config, &jobs, input, actor, is_listener).await
        }
    }
}

fn valid_without_listener(input: &AcceptedObjects) -> bool {
    match input.kind {
        ValidTypes::Follow => true,
        ValidTypes::Undo if input.object.is_kind("Follow") => true,
        _ => false,
    }
}

async fn handle_accept(config: &Config, input: AcceptedObjects) -> Result<HttpResponse, MyError> {
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

    Ok(accepted(serde_json::json!({})))
}

async fn handle_reject(
    db: &Db,
    config: &Config,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
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

    let my_id: XsdAnyUri = config.generate_url(UrlKind::Actor).parse()?;

    let inbox = actor.inbox().to_owned();
    db.remove_listener(inbox).await?;

    let undo = generate_undo_follow(config, &actor.id, &my_id)?;

    let inbox = actor.inbox().to_owned();
    jobs.queue(Deliver::new(inbox, undo.clone())?)?;

    Ok(accepted(undo))
}

async fn handle_undo(
    db: &Db,
    state: &State,
    config: &Config,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: AcceptedActors,
    is_listener: bool,
) -> Result<HttpResponse, MyError> {
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
            return handle_forward(state, jobs, input, actor).await;
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
        return Ok(accepted(serde_json::json!({})));
    }

    let inbox = actor.inbox().to_owned();
    db.remove_listener(inbox).await?;

    let undo = generate_undo_follow(config, &actor.id, &my_id)?;

    let inbox = actor.inbox().to_owned();
    jobs.queue(Deliver::new(inbox, undo.clone())?)?;

    Ok(accepted(undo))
}

async fn handle_forward(
    state: &State,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let object_id = input.object.id();

    let inboxes = get_inboxes(state, &actor, &object_id).await?;
    jobs.queue(DeliverMany::new(inboxes, input.clone())?)?;

    Ok(accepted(input))
}

async fn handle_announce(
    state: &State,
    config: &Config,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let object_id = input.object.id();

    if state.is_cached(object_id).await {
        return Err(MyError::Duplicate);
    }

    let activity_id: XsdAnyUri = config.generate_url(UrlKind::Activity).parse()?;

    let announce = generate_announce(config, &activity_id, object_id)?;
    let inboxes = get_inboxes(state, &actor, &object_id).await?;
    jobs.queue(DeliverMany::new(inboxes, announce.clone())?)?;

    state.cache(object_id.to_owned(), activity_id).await;

    Ok(accepted(announce))
}

async fn handle_follow(
    db: &Db,
    config: &Config,
    jobs: &JobServer,
    input: AcceptedObjects,
    actor: AcceptedActors,
    is_listener: bool,
) -> Result<HttpResponse, MyError> {
    let my_id: XsdAnyUri = config.generate_url(UrlKind::Actor).parse()?;

    if !input.object.is(&my_id) && !input.object.is(&public()) {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    if !is_listener {
        let inbox = actor.inbox().to_owned();
        db.add_listener(inbox).await?;

        // if following relay directly, not just following 'public', followback
        if input.object.is(&my_id) {
            let follow = generate_follow(config, &actor.id, &my_id)?;
            let inbox = actor.inbox().to_owned();
            jobs.queue(Deliver::new(inbox, follow)?)?;
        }
    }

    let accept = generate_accept_follow(config, &actor.id, &input.id, &my_id)?;

    let inbox = actor.inbox().to_owned();
    jobs.queue(Deliver::new(inbox, accept.clone())?)?;

    Ok(accepted(accept))
}

// Generate a type that says "I want to stop following you"
fn generate_undo_follow(
    config: &Config,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<Undo, MyError> {
    let mut undo = Undo::default();

    undo.undo_props
        .set_actor_xsd_any_uri(my_id.clone())?
        .set_object_base_box({
            let mut follow = Follow::default();

            follow
                .object_props
                .set_id(config.generate_url(UrlKind::Activity))?;
            follow
                .follow_props
                .set_actor_xsd_any_uri(actor_id.clone())?
                .set_object_xsd_any_uri(actor_id.clone())?;

            follow
        })?;

    prepare_activity(undo, config.generate_url(UrlKind::Actor), actor_id.clone())
}

// Generate a type that says "Look at this object"
fn generate_announce(
    config: &Config,
    activity_id: &XsdAnyUri,
    object_id: &XsdAnyUri,
) -> Result<Announce, MyError> {
    let mut announce = Announce::default();

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

// Generate a type that says "I want to follow you"
fn generate_follow(
    config: &Config,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<Follow, MyError> {
    let mut follow = Follow::default();

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
) -> Result<Accept, MyError> {
    let mut accept = Accept::default();

    accept
        .accept_props
        .set_actor_xsd_any_uri(my_id.clone())?
        .set_object_base_box({
            let mut follow = Follow::default();

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

fn prepare_activity<T, U, V>(
    mut t: T,
    id: impl TryInto<XsdAnyUri, Error = U>,
    to: impl TryInto<XsdAnyUri, Error = V>,
) -> Result<T, MyError>
where
    T: AsMut<ObjectProperties>,
    MyError: From<U> + From<V>,
{
    t.as_mut()
        .set_id(id.try_into()?)?
        .set_many_to_xsd_any_uris(vec![to.try_into()?])?
        .set_many_context_xsd_any_uris(vec![context(), security()])?;
    Ok(t)
}

async fn get_inboxes(
    state: &State,
    actor: &AcceptedActors,
    object_id: &XsdAnyUri,
) -> Result<Vec<XsdAnyUri>, MyError> {
    let domain = object_id
        .as_url()
        .host()
        .ok_or(MyError::Domain)?
        .to_string();

    let inbox = actor.inbox();

    Ok(state.listeners_without(&inbox, &domain).await)
}
