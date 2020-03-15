use activitystreams::primitives::XsdAnyUri;
use actix::Addr;
use actix_web::{client::Client, web, Responder};
use log::info;

use crate::{
    apub::{AcceptedActors, AcceptedObjects, ValidTypes},
    db_actor::DbActor,
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

    info!("Relaying {} for {}", input.object.id(), input.actor);
    let actor = fetch_actor(state, client, &input.actor).await?;
    info!("Actor, {:#?}", actor);

    match input.kind {
        ValidTypes::Announce => (),
        ValidTypes::Create => (),
        ValidTypes::Delete => (),
        ValidTypes::Follow => (),
        ValidTypes::Undo => (),
    }

    Ok("{}")
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
        .get(actor_id.as_ref())
        .header("Accept", "application/activity+json")
        .send()
        .await
        .map_err(|_| MyError)?
        .json()
        .await
        .map_err(|_| MyError)?;

    state.cache_actor(actor_id.to_owned(), actor.clone()).await;

    Ok(actor)
}

impl actix_web::error::ResponseError for MyError {}
