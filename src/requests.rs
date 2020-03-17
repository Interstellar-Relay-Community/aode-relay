use crate::{
    apub::AcceptedActors,
    error::MyError,
    state::{State, UrlKind},
};
use activitystreams::primitives::XsdAnyUri;
use actix_web::{client::Client, web};
use log::error;

pub async fn fetch_actor(
    state: std::sync::Arc<State>,
    client: std::sync::Arc<Client>,
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
            MyError::SendRequest
        })?
        .json()
        .await
        .map_err(|e| {
            error!("Coudn't fetch actor from {}, {}", actor_id, e);
            MyError::ReceiveResponse
        })?;

    state.cache_actor(actor_id.to_owned(), actor.clone()).await;

    Ok(actor)
}

pub fn deliver_many<T>(
    state: web::Data<State>,
    client: web::Data<Client>,
    inboxes: Vec<XsdAnyUri>,
    item: T,
) where
    T: serde::ser::Serialize + 'static,
{
    let client = client.into_inner();
    let state = state.into_inner();

    actix::Arbiter::spawn(async move {
        use futures::stream::StreamExt;

        let mut unordered = futures::stream::FuturesUnordered::new();

        for inbox in inboxes {
            unordered.push(deliver(&state, &client, inbox, &item));
        }

        while let Some(_) = unordered.next().await {}
    });
}

pub async fn deliver<T>(
    state: &std::sync::Arc<State>,
    client: &std::sync::Arc<Client>,
    inbox: XsdAnyUri,
    item: &T,
) -> Result<(), MyError>
where
    T: serde::ser::Serialize,
{
    use http_signature_normalization_actix::prelude::*;
    use sha2::{Digest, Sha256};

    let config = Config::default();
    let mut digest = Sha256::new();

    let key_id = state.generate_url(UrlKind::Actor);

    let item_string = serde_json::to_string(item)?;

    let res = client
        .post(inbox.as_str())
        .header("Accept", "application/activity+json")
        .header("Content-Type", "application/activity+json")
        .header("User-Agent", "Aode Relay v0.1.0")
        .signature_with_digest(
            &config,
            &key_id,
            &mut digest,
            item_string,
            |signing_string| state.sign(signing_string),
        )?
        .send()
        .await
        .map_err(|e| {
            error!("Couldn't send deliver request to {}, {}", inbox, e);
            MyError::SendRequest
        })?;

    if !res.status().is_success() {
        error!("Invalid response status from {}, {}", inbox, res.status());
        return Err(MyError::Status);
    }

    Ok(())
}
