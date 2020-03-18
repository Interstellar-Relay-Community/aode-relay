use crate::{
    apub::{AcceptedActors, AcceptedObjects, ValidTypes},
    db_actor::Db,
    error::MyError,
    requests::Requests,
    response,
    state::{State, UrlKind},
};
use activitystreams::{
    activity::apub::{Accept, Announce, Follow, Undo},
    context,
    primitives::XsdAnyUri,
};
use actix_web::{web, HttpResponse};
use futures::join;
use http_signature_normalization_actix::middleware::SignatureVerified;
use log::error;

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
        state.is_listener(&actor.id)
    );

    if is_blocked {
        return Err(MyError::Blocked(actor.id.to_string()));
    }

    if !is_whitelisted {
        return Err(MyError::Whitelist(actor.id.to_string()));
    }

    if input.kind != ValidTypes::Follow && !is_listener {
        return Err(MyError::NotSubscribed(actor.id.to_string()));
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
            handle_relay(&state, &client, input, actor).await
        }
        ValidTypes::Follow => handle_follow(&db, &state, &client, input, actor).await,
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
    if !input.object.is_kind("Follow") {
        return Err(MyError::Kind(
            input.object.kind().unwrap_or("unknown").to_owned(),
        ));
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

    Ok(response(undo))
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

    Ok(response(input))
}

async fn handle_relay(
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

    Ok(response(announce))
}

async fn handle_follow(
    db: &Db,
    state: &State,
    client: &Requests,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let my_id: XsdAnyUri = state.generate_url(UrlKind::Actor).parse()?;

    if !input.object.is(&my_id) {
        return Err(MyError::WrongActor(input.object.id().to_string()));
    }

    let is_listener = state.is_listener(&actor.id).await;

    if !is_listener {
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

    Ok(response(accept))
}

// Generate a type that says "I want to stop following you"
fn generate_undo_follow(
    state: &State,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<Undo, MyError> {
    let mut undo = Undo::default();
    let mut follow = Follow::default();

    follow
        .object_props
        .set_id(state.generate_url(UrlKind::Activity))?;
    follow
        .follow_props
        .set_actor_xsd_any_uri(actor_id.clone())?
        .set_object_xsd_any_uri(actor_id.clone())?;

    undo.object_props
        .set_id(state.generate_url(UrlKind::Activity))?
        .set_many_to_xsd_any_uris(vec![actor_id.clone()])?
        .set_context_xsd_any_uri(context())?;
    undo.undo_props
        .set_object_object_box(follow)?
        .set_actor_xsd_any_uri(my_id.clone())?;

    Ok(undo)
}

// Generate a type that says "Look at this object"
fn generate_announce(
    state: &State,
    activity_id: &XsdAnyUri,
    object_id: &XsdAnyUri,
) -> Result<Announce, MyError> {
    let mut announce = Announce::default();

    announce
        .object_props
        .set_context_xsd_any_uri(context())?
        .set_many_to_xsd_any_uris(vec![state.generate_url(UrlKind::Followers)])?
        .set_id(activity_id.clone())?;

    announce
        .announce_props
        .set_object_xsd_any_uri(object_id.clone())?
        .set_actor_xsd_any_uri(state.generate_url(UrlKind::Actor))?;

    Ok(announce)
}

// Generate a type that says "I want to follow you"
fn generate_follow(
    state: &State,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<Follow, MyError> {
    let mut follow = Follow::default();

    follow
        .object_props
        .set_id(state.generate_url(UrlKind::Activity))?
        .set_many_to_xsd_any_uris(vec![actor_id.clone()])?
        .set_context_xsd_any_uri(context())?;

    follow
        .follow_props
        .set_object_xsd_any_uri(actor_id.clone())?
        .set_actor_xsd_any_uri(my_id.clone())?;

    Ok(follow)
}

// Generate a type that says "I accept your follow request"
fn generate_accept_follow(
    state: &State,
    actor_id: &XsdAnyUri,
    input_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<Accept, MyError> {
    let mut accept = Accept::default();
    let mut follow = Follow::default();

    follow.object_props.set_id(input_id.clone())?;
    follow
        .follow_props
        .set_object_xsd_any_uri(my_id.clone())?
        .set_actor_xsd_any_uri(actor_id.clone())?;

    accept
        .object_props
        .set_id(state.generate_url(UrlKind::Activity))?
        .set_many_to_xsd_any_uris(vec![actor_id.clone()])?;
    accept
        .accept_props
        .set_object_object_box(follow)?
        .set_actor_xsd_any_uri(my_id.clone())?;

    Ok(accept)
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
