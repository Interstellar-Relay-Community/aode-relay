use activitystreams::primitives::XsdAnyUri;
use actix::Addr;
use actix_web::{client::Client, web, Responder};
use log::info;
use std::sync::Arc;

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
    let _state = state.into_inner();
    let input = input.into_inner();

    info!("Relaying {} for {}", input.object.id(), input.actor);
    let actor = fetch_actor(client.into_inner(), &input.actor).await?;
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

async fn fetch_actor(client: Arc<Client>, actor_id: &XsdAnyUri) -> Result<AcceptedActors, MyError> {
    client
        .get(actor_id.as_ref())
        .header("Accept", "application/activity+json")
        .send()
        .await
        .map_err(|_| MyError)?
        .json()
        .await
        .map_err(|_| MyError)
}

impl actix_web::error::ResponseError for MyError {}
