use crate::{
    apub::{AcceptedActors, AcceptedObjects, ValidTypes},
    db::{add_listener, remove_listener},
    db_actor::{DbActor, DbQuery, Pool},
    error::MyError,
    requests::{deliver, deliver_many, fetch_actor},
    response,
    state::{State, UrlKind},
};
use activitystreams::{
    activity::apub::{Accept, Announce, Follow, Undo},
    context,
    primitives::XsdAnyUri,
};
use actix::Addr;
use actix_web::{client::Client, web, HttpResponse};
use futures::join;
use http_signature_normalization_actix::middleware::SignatureVerified;
use log::error;

pub async fn inbox(
    db_actor: web::Data<Addr<DbActor>>,
    state: web::Data<State>,
    client: web::Data<Client>,
    input: web::Json<AcceptedObjects>,
    verified: SignatureVerified,
) -> Result<HttpResponse, MyError> {
    let input = input.into_inner();

    let actor = fetch_actor(
        state.clone().into_inner(),
        client.clone().into_inner(),
        &input.actor,
    )
    .await?;

    if actor.public_key.id.as_str() != verified.key_id() {
        error!(
            "Request payload and requestor disagree on actor, {} != {}",
            input.actor,
            verified.key_id()
        );
        return Err(MyError::BadActor);
    }

    match input.kind {
        ValidTypes::Announce | ValidTypes::Create => {
            handle_relay(state, client, input, actor).await
        }
        ValidTypes::Follow => handle_follow(db_actor, state, client, input, actor).await,
        ValidTypes::Delete | ValidTypes::Update => {
            handle_forward(state, client, input, actor).await
        }
        ValidTypes::Undo => handle_undo(db_actor, state, client, input, actor).await,
    }
}

async fn handle_undo(
    db_actor: web::Data<Addr<DbActor>>,
    state: web::Data<State>,
    client: web::Data<Client>,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    if !input.object.is_kind("Follow") {
        return Err(MyError::Kind);
    }

    let inbox = actor.inbox().to_owned();

    db_actor.do_send(DbQuery(move |pool: Pool| {
        let inbox = inbox.clone();

        async move {
            let conn = pool.get().await?;

            remove_listener(&conn, &inbox).await.map_err(|e| {
                error!("Error removing listener, {}", e);
                e
            })
        }
    }));

    let mut undo = Undo::default();
    let mut follow = Follow::default();

    follow
        .object_props
        .set_id(state.generate_url(UrlKind::Activity))?;
    follow
        .follow_props
        .set_actor_xsd_any_uri(actor.id.clone())?
        .set_object_xsd_any_uri(actor.id.clone())?;

    undo.object_props
        .set_id(state.generate_url(UrlKind::Activity))?
        .set_many_to_xsd_any_uris(vec![actor.id.clone()])?
        .set_context_xsd_any_uri(context())?;
    undo.undo_props
        .set_object_object_box(follow)?
        .set_actor_xsd_any_uri(state.generate_url(UrlKind::Actor))?;

    if input.object.child_object_is_actor() {
        let undo2 = undo.clone();
        let client = client.into_inner();
        actix::Arbiter::spawn(async move {
            let _ = deliver(&state.into_inner(), &client, actor.id, &undo2).await;
        });
    }

    Ok(response(undo))
}

async fn handle_forward(
    state: web::Data<State>,
    client: web::Data<Client>,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let object_id = input.object.id();

    let inboxes = get_inboxes(&state, &actor, &object_id).await?;

    deliver_many(state, client, inboxes, input.clone());

    Ok(response(input))
}

async fn handle_relay(
    state: web::Data<State>,
    client: web::Data<Client>,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let object_id = input.object.id();

    if state.is_cached(object_id).await {
        return Err(MyError::Duplicate);
    }

    let activity_id: XsdAnyUri = state.generate_url(UrlKind::Activity).parse()?;

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

    let inboxes = get_inboxes(&state, &actor, &object_id).await?;

    state.cache(object_id.to_owned(), activity_id).await;

    deliver_many(state, client, inboxes, announce.clone());

    Ok(response(announce))
}

async fn handle_follow(
    db_actor: web::Data<Addr<DbActor>>,
    state: web::Data<State>,
    client: web::Data<Client>,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let (is_listener, is_blocked, is_whitelisted) = join!(
        state.is_listener(&actor.id),
        state.is_blocked(&actor.id),
        state.is_whitelisted(&actor.id)
    );

    if is_blocked {
        error!("Follow from blocked listener, {}", actor.id);
        return Err(MyError::Blocked);
    }

    if !is_whitelisted {
        error!("Follow from non-whitelisted listener, {}", actor.id);
        return Err(MyError::Whitelist);
    }

    if !is_listener {
        let inbox = actor.inbox().to_owned();
        db_actor.do_send(DbQuery(move |pool: Pool| {
            let inbox = inbox.clone();

            async move {
                let conn = pool.get().await?;

                add_listener(&conn, &inbox).await.map_err(|e| {
                    error!("Error adding listener, {}", e);
                    e
                })
            }
        }));
    }

    let actor_inbox = actor.inbox().clone();

    let mut accept = Accept::default();
    let mut follow = Follow::default();
    follow.object_props.set_id(input.id)?;
    follow
        .follow_props
        .set_object_xsd_any_uri(state.generate_url(UrlKind::Actor))?
        .set_actor_xsd_any_uri(actor.id.clone())?;

    accept
        .object_props
        .set_id(state.generate_url(UrlKind::Activity))?
        .set_many_to_xsd_any_uris(vec![actor.id])?;
    accept
        .accept_props
        .set_object_object_box(follow)?
        .set_actor_xsd_any_uri(state.generate_url(UrlKind::Actor))?;

    let client = client.into_inner();
    let accept2 = accept.clone();
    actix::Arbiter::spawn(async move {
        let _ = deliver(&state.into_inner(), &client.clone(), actor_inbox, &accept2).await;
    });

    Ok(response(accept))
}

async fn get_inboxes(
    state: &web::Data<State>,
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
