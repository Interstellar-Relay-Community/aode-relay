use activitystreams::{
    activity::apub::{Accept, Announce, Follow, Undo},
    context,
    primitives::XsdAnyUri,
};
use actix::Addr;
use actix_web::{client::Client, web, HttpResponse};
use futures::join;
use log::error;
use std::collections::HashMap;

use crate::{
    apub::{AcceptedActors, AcceptedObjects, ValidTypes},
    db_actor::{DbActor, DbQuery, Pool},
    state::{State, UrlKind},
};

#[derive(Clone, Debug, thiserror::Error)]
#[error("Something went wrong :(")]
pub struct MyError;

pub async fn inbox(
    db_actor: web::Data<Addr<DbActor>>,
    state: web::Data<State>,
    client: web::Data<Client>,
    input: web::Json<AcceptedObjects>,
) -> Result<HttpResponse, MyError> {
    let input = input.into_inner();

    let actor = fetch_actor(state.clone(), &client, &input.actor).await?;

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

pub fn response<T>(item: T) -> HttpResponse
where
    T: serde::ser::Serialize,
{
    HttpResponse::Accepted()
        .content_type("application/activity+json")
        .json(item)
}

async fn handle_undo(
    db_actor: web::Data<Addr<DbActor>>,
    state: web::Data<State>,
    client: web::Data<Client>,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    if !input.object.is_kind("Follow") {
        return Err(MyError);
    }

    let inbox = actor.inbox().to_owned();

    let state2 = state.clone().into_inner();
    db_actor.do_send(DbQuery(move |pool: Pool| {
        let inbox = inbox.clone();

        async move {
            let conn = pool.get().await?;

            state2.remove_listener(&conn, &inbox).await.map_err(|e| {
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
            let _ = deliver(&client, actor.id, &undo2).await;
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

    deliver_many(client, inboxes, input);

    Ok(response(HashMap::<(), ()>::new()))
}

async fn handle_relay(
    state: web::Data<State>,
    client: web::Data<Client>,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<HttpResponse, MyError> {
    let object_id = input.object.id();

    if state.is_cached(object_id).await {
        return Err(MyError);
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

    deliver_many(client, inboxes, announce);

    state.cache(object_id.to_owned(), activity_id).await;

    Ok(response(HashMap::<(), ()>::new()))
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
        return Err(MyError);
    }

    if !is_whitelisted {
        error!("Follow from non-whitelisted listener, {}", actor.id);
        return Err(MyError);
    }

    if !is_listener {
        let state = state.clone().into_inner();

        let inbox = actor.inbox().to_owned();
        db_actor.do_send(DbQuery(move |pool: Pool| {
            let inbox = inbox.clone();
            let state = state.clone();

            async move {
                let conn = pool.get().await?;

                state.add_listener(&conn, inbox).await.map_err(|e| {
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
        let _ = deliver(&client, actor_inbox, &accept2).await;
    });

    Ok(response(accept))
}

async fn fetch_actor(
    state: web::Data<State>,
    client: &web::Data<Client>,
    actor_id: &XsdAnyUri,
) -> Result<AcceptedActors, MyError> {
    if let Some(actor) = state.get_actor(actor_id).await {
        return Ok(actor);
    }

    let actor: AcceptedActors = client
        .get(actor_id.as_str())
        .header("Accept", "application/activity+json")
        .send()
        .await
        .map_err(|e| {
            error!("Couldn't send request to {} for actor, {}", actor_id, e);
            MyError
        })?
        .json()
        .await
        .map_err(|e| {
            error!("Coudn't fetch actor from {}, {}", actor_id, e);
            MyError
        })?;

    state.cache_actor(actor_id.to_owned(), actor.clone()).await;

    Ok(actor)
}

fn deliver_many<T>(client: web::Data<Client>, inboxes: Vec<XsdAnyUri>, item: T)
where
    T: serde::ser::Serialize + 'static,
{
    let client = client.into_inner();

    actix::Arbiter::spawn(async move {
        use futures::stream::StreamExt;

        let client = client.clone();
        let mut unordered = futures::stream::FuturesUnordered::new();

        for inbox in inboxes {
            unordered.push(deliver(&client, inbox, &item));
        }

        while let Some(_) = unordered.next().await {}
    });
}

async fn deliver<T>(
    client: &std::sync::Arc<Client>,
    inbox: XsdAnyUri,
    item: &T,
) -> Result<(), MyError>
where
    T: serde::ser::Serialize,
{
    let res = client
        .post(inbox.as_str())
        .header("Accept", "application/activity+json")
        .header("Content-Type", "application/activity+json")
        .send_json(item)
        .await
        .map_err(|e| {
            error!("Couldn't send deliver request to {}, {}", inbox, e);
            MyError
        })?;

    if !res.status().is_success() {
        error!("Invalid response status from {}, {}", inbox, res.status());
        return Err(MyError);
    }

    Ok(())
}

async fn get_inboxes(
    state: &web::Data<State>,
    actor: &AcceptedActors,
    object_id: &XsdAnyUri,
) -> Result<Vec<XsdAnyUri>, MyError> {
    let domain = object_id.as_url().host().ok_or(MyError)?.to_string();

    let inbox = actor.inbox();

    Ok(state.listeners_without(&inbox, &domain).await)
}

impl actix_web::error::ResponseError for MyError {}

impl From<std::convert::Infallible> for MyError {
    fn from(_: std::convert::Infallible) -> Self {
        MyError
    }
}

impl From<activitystreams::primitives::XsdAnyUriError> for MyError {
    fn from(_: activitystreams::primitives::XsdAnyUriError) -> Self {
        error!("Error parsing URI");
        MyError
    }
}

impl From<std::io::Error> for MyError {
    fn from(e: std::io::Error) -> Self {
        error!("JSON Error, {}", e);
        MyError
    }
}
