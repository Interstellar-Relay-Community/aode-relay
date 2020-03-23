use crate::{
    data::{ActorCache, State},
    db::listen,
    error::MyError,
    jobs::{JobServer, QueryInstance, QueryNodeinfo},
};
use activitystreams::primitives::XsdAnyUri;
use actix::clock::{delay_for, Duration};
use bb8_postgres::tokio_postgres::{tls::NoTls, AsyncMessage, Config, Notification};
use futures::{
    future::ready,
    stream::{poll_fn, StreamExt},
};
use log::{debug, error, info, warn};
use std::sync::Arc;

async fn handle_notification(
    state: State,
    actors: ActorCache,
    job_server: JobServer,
    notif: Notification,
) {
    match notif.channel() {
        "new_blocks" => {
            info!("Caching block of {}", notif.payload());
            state.cache_block(notif.payload().to_owned()).await;
        }
        "new_whitelists" => {
            info!("Caching whitelist of {}", notif.payload());
            state.cache_whitelist(notif.payload().to_owned()).await;
        }
        "new_listeners" => {
            if let Ok(uri) = notif.payload().parse::<XsdAnyUri>() {
                info!("Caching listener {}", uri);
                state.cache_listener(uri.clone()).await;
                let _ = job_server.queue_local(QueryInstance::new(uri.clone()));
                let _ = job_server.queue_local(QueryNodeinfo::new(uri));
            }
        }
        "new_actors" => {
            if let Ok(uri) = notif.payload().parse::<XsdAnyUri>() {
                info!("Caching follower {}", uri);
                actors.cache_follower(uri).await;
            }
        }
        "rm_blocks" => {
            info!("Busting block cache for {}", notif.payload());
            state.bust_block(notif.payload()).await;
        }
        "rm_whitelists" => {
            info!("Busting whitelist cache for {}", notif.payload());
            state.bust_whitelist(notif.payload()).await;
        }
        "rm_listeners" => {
            if let Ok(uri) = notif.payload().parse::<XsdAnyUri>() {
                info!("Busting listener cache for {}", uri);
                state.bust_listener(&uri).await;
            }
        }
        "rm_actors" => {
            if let Ok(uri) = notif.payload().parse::<XsdAnyUri>() {
                info!("Busting follower cache for {}", uri);
                actors.bust_follower(&uri).await;
            }
        }
        _ => (),
    };
}

pub fn spawn(
    state: State,
    actors: ActorCache,
    job_server: JobServer,
    config: &crate::config::Config,
) -> Result<(), MyError> {
    let config: Config = config.database_url().parse()?;

    actix::spawn(async move {
        loop {
            let (new_client, mut conn) = match config.connect(NoTls).await {
                Ok((client, conn)) => (client, conn),
                Err(e) => {
                    error!("Error establishing DB Connection, {}", e);
                    delay_for(Duration::new(5, 0)).await;
                    continue;
                }
            };

            let client = Arc::new(new_client);
            let new_client = client.clone();

            actix::spawn(async move {
                if let Err(e) = listen(&new_client).await {
                    error!("Error listening for updates, {}", e);
                }
            });

            let mut stream = poll_fn(move |cx| conn.poll_message(cx)).filter_map(|m| match m {
                Ok(AsyncMessage::Notification(n)) => {
                    debug!("Handling Notification, {:?}", n);
                    ready(Some(n))
                }
                Ok(AsyncMessage::Notice(e)) => {
                    debug!("Handling Notice, {:?}", e);
                    ready(None)
                }
                Err(e) => {
                    debug!("Handling Error, {:?}", e);
                    ready(None)
                }
                _ => {
                    debug!("Handling rest");
                    ready(None)
                }
            });

            while let Some(n) = stream.next().await {
                actix::spawn(handle_notification(
                    state.clone(),
                    actors.clone(),
                    job_server.clone(),
                    n,
                ));
            }

            drop(client);
            warn!("Restarting listener task");
        }
    });
    Ok(())
}
