use crate::{
    accepted,
    apub::{AcceptedActors, AcceptedObjects, ValidTypes},
    db_actor::Db,
    error::MyError,
    requests::Requests,
    state::{State, UrlKind},
};
use activitystreams::{
    activity::apub::{Accept, Announce, Follow, Undo},
    context,
    object::properties::ObjectProperties,
    primitives::XsdAnyUri,
};
use actix_web::{web, HttpResponse};
use futures::join;
use http_signature_normalization_actix::middleware::SignatureVerified;
use log::error;
use std::convert::TryInto;

fn public() -> XsdAnyUri {
    "https://www.w3.org/ns/activitystreams#Public"
        .parse()
        .unwrap()
}

pub async fn inbox(
    db: web::Data<Db>,
    state: web::Data<State>,
    client: web::Data<Requests>,
    input: web::Json<AcceptedObjects>,
    verified: SignatureVerified,
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

    if input.kind != ValidTypes::Follow && !is_listener {
        return Err(MyError::NotSubscribed(actor.inbox().to_string()));
    }

    if actor.public_key.id.as_str() != verified.key_id() {
        error!("Bad actor, more info: {:?}", input);
        return Err(MyError::BadActor(
            actor.public_key.id.to_string(),
            verified.key_id().to_owned(),
        ));
    }

    match input.kind {
        ValidTypes::Announce | ValidTypes::Create => {
            handle_announce(&state, &client, input, actor).await
        }
        ValidTypes::Follow => handle_follow(&db, &state, &client, input, actor, is_listener).await,
        ValidTypes::Delete | ValidTypes::Update => {
            handle_forward(&state, &client, input, actor).await
        }
        ValidTypes::Undo => handle_undo(&db, &state, &client, input, actor).await,
    }
}

async fn handle_undo(
    db: &Db,
    state: &State,
    client: &Requests,
    input: AcceptedObjects,
    actor: AcceptedActors,
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
        return handle_forward(state, client, input, actor).await;
    }

    let my_id: XsdAnyUri = state.generate_url(UrlKind::Actor).parse()?;

    if !input.object.child_object_is(&my_id) && !input.object.child_object_is(&public()) {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    let inbox = actor.inbox().to_owned();
    db.remove_listener(inbox);

    let undo = generate_undo_follow(state, &actor.id, &my_id)?;

    let client2 = client.clone();
    let inbox = actor.inbox().clone();
    let undo2 = undo.clone();
    actix::Arbiter::spawn(async move {
        let _ = client2.deliver(inbox, &undo2).await;
    });

    Ok(accepted(undo))
}

async fn handle_forward(
    state: &State,
    client: &Requests,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let object_id = input.object.id();

    let inboxes = get_inboxes(state, &actor, &object_id).await?;
    client.deliver_many(inboxes, input.clone());

    Ok(accepted(input))
}

async fn handle_announce(
    state: &State,
    client: &Requests,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let object_id = input.object.id();

    if state.is_cached(object_id).await {
        return Err(MyError::Duplicate);
    }

    let activity_id: XsdAnyUri = state.generate_url(UrlKind::Activity).parse()?;

    let announce = generate_announce(state, &activity_id, object_id)?;
    let inboxes = get_inboxes(state, &actor, &object_id).await?;
    client.deliver_many(inboxes, announce.clone());

    state.cache(object_id.to_owned(), activity_id).await;

    Ok(accepted(announce))
}

async fn handle_follow(
    db: &Db,
    state: &State,
    client: &Requests,
    input: AcceptedObjects,
    actor: AcceptedActors,
    is_listener: bool,
) -> Result<HttpResponse, MyError> {
    let my_id: XsdAnyUri = state.generate_url(UrlKind::Actor).parse()?;

    if !input.object.is(&my_id) && !input.object.is(&public()) {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    if !is_listener && input.object.is(&my_id) {
        let follow = generate_follow(state, &actor.id, &my_id)?;

        let inbox = actor.inbox().to_owned();
        db.add_listener(inbox);

        let client2 = client.clone();
        let inbox = actor.inbox().clone();
        let follow2 = follow.clone();
        actix::Arbiter::spawn(async move {
            let _ = client2.deliver(inbox, &follow2).await;
        });
    }

    let accept = generate_accept_follow(state, &actor.id, &input.id, &my_id)?;

    let client2 = client.clone();
    let inbox = actor.inbox().clone();
    let accept2 = accept.clone();
    actix::Arbiter::spawn(async move {
        let _ = client2.deliver(inbox, &accept2).await;
    });

    Ok(accepted(accept))
}

// Generate a type that says "I want to stop following you"
fn generate_undo_follow(
    state: &State,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<Undo, MyError> {
    let mut undo = Undo::default();

    undo.undo_props
        .set_actor_xsd_any_uri(my_id.clone())?
        .set_object_object_box({
            let mut follow = Follow::default();

            follow
                .object_props
                .set_id(state.generate_url(UrlKind::Activity))?;
            follow
                .follow_props
                .set_actor_xsd_any_uri(actor_id.clone())?
                .set_object_xsd_any_uri(actor_id.clone())?;

            follow
        })?;

    prepare_activity(undo, state.generate_url(UrlKind::Actor), actor_id.clone())
}

// Generate a type that says "Look at this object"
fn generate_announce(
    state: &State,
    activity_id: &XsdAnyUri,
    object_id: &XsdAnyUri,
) -> Result<Announce, MyError> {
    let mut announce = Announce::default();

    announce
        .announce_props
        .set_object_xsd_any_uri(object_id.clone())?
        .set_actor_xsd_any_uri(state.generate_url(UrlKind::Actor))?;

    prepare_activity(
        announce,
        activity_id.clone(),
        state.generate_url(UrlKind::Followers),
    )
}

// Generate a type that says "I want to follow you"
fn generate_follow(
    state: &State,
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
        state.generate_url(UrlKind::Activity),
        actor_id.clone(),
    )
}

// Generate a type that says "I accept your follow request"
fn generate_accept_follow(
    state: &State,
    actor_id: &XsdAnyUri,
    input_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<Accept, MyError> {
    let mut accept = Accept::default();

    accept
        .accept_props
        .set_actor_xsd_any_uri(my_id.clone())?
        .set_object_object_box({
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
        state.generate_url(UrlKind::Activity),
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
        .set_context_xsd_any_uri(context())?;
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
