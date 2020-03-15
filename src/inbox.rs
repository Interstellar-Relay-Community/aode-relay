use activitystreams::{
    activity::apub::{Accept, Follow},
    primitives::XsdAnyUri,
};
use actix::Addr;
use actix_web::{client::Client, web, Responder};
use futures::join;
use log::{error, info};

use crate::{
    apub::{AcceptedActors, AcceptedObjects, ValidTypes},
    db_actor::{DbActor, DbQuery, Pool},
    state::State,
};

#[derive(Clone, Debug, thiserror::Error)]
#[error("Something went wrong :(")]
pub struct MyError;

pub async fn inbox(
    db_actor: web::Data<Addr<DbActor>>,
    state: web::Data<State>,
    client: web::Data<Client>,
    input: web::Json<AcceptedObjects>,
) -> Result<impl Responder, MyError> {
    let input = input.into_inner();

    let actor = fetch_actor(state.clone(), client, &input.actor).await?;

    match input.kind {
        ValidTypes::Announce => (),
        ValidTypes::Create => (),
        ValidTypes::Delete => (),
        ValidTypes::Follow => return handle_follow(db_actor, state, input, actor).await,
        ValidTypes::Undo => (),
    }

    Err(MyError)
}

async fn handle_follow(
    db_actor: web::Data<Addr<DbActor>>,
    state: web::Data<State>,
    input: AcceptedObjects,
    actor: AcceptedActors,
) -> Result<web::Json<Accept>, MyError> {
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
        let state = state.into_inner();

        let actor = actor.clone();
        db_actor.do_send(DbQuery(move |pool: Pool| {
            let actor_id = actor.id.clone();
            let state = state.clone();

            async move {
                let conn = pool.get().await?;

                state.add_listener(&conn, actor_id).await
            }
        }));
    }

    let mut accept = Accept::default();
    let mut follow = Follow::default();
    follow.object_props.set_id(input.id)?;
    follow
        .follow_props
        .set_object_xsd_any_uri(format!("https://{}/actor", "localhost"))?
        .set_actor_xsd_any_uri(actor.id.clone())?;

    accept
        .object_props
        .set_id(format!("https://{}/activities/{}", "localhost", "1"))?
        .set_many_to_xsd_any_uris(vec![actor.id])?;
    accept
        .accept_props
        .set_object_object_box(follow)?
        .set_actor_xsd_any_uri(format!("https://{}/actor", "localhost"))?;

    Ok(web::Json(accept))
}

async fn fetch_actor(
    state: web::Data<State>,
    client: web::Data<Client>,
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
            error!("Couldn't send request for actor, {}", e);
            MyError
        })?
        .json()
        .await
        .map_err(|e| {
            error!("Coudn't fetch actor, {}", e);
            MyError
        })?;

    state.cache_actor(actor_id.to_owned(), actor.clone()).await;

    Ok(actor)
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
