use crate::{
    data::{ActorCache, NodeCache, State},
    db::listen,
    jobs::{JobServer, QueryInstance, QueryNodeinfo},
};
use activitystreams::url::Url;
use actix_rt::{spawn, time::delay_for};
use futures::stream::{poll_fn, StreamExt};
use log::{debug, error, warn};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio_postgres::{tls::NoTls, AsyncMessage, Config};
use uuid::Uuid;

pub trait Listener {
    fn key(&self) -> &str;

    fn execute(&self, payload: &str);
}

pub struct Notifier {
    config: Config,
    listeners: HashMap<String, Vec<Box<dyn Listener + Send + Sync + 'static>>>,
}

impl Notifier {
    pub fn new(config: Config) -> Self {
        Notifier {
            config,
            listeners: HashMap::new(),
        }
    }

    pub fn register<L>(mut self, l: L) -> Self
    where
        L: Listener + Send + Sync + 'static,
    {
        let v = self
            .listeners
            .entry(l.key().to_owned())
            .or_insert_with(Vec::new);
        v.push(Box::new(l));
        self
    }

    pub fn start(self) {
        spawn(async move {
            let Notifier { config, listeners } = self;

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

                spawn(async move {
                    if let Err(e) = listen(&new_client).await {
                        error!("Error listening for updates, {}", e);
                    }
                });

                let mut stream = poll_fn(move |cx| conn.poll_message(cx));

                loop {
                    match stream.next().await {
                        Some(Ok(AsyncMessage::Notification(n))) => {
                            debug!("Handling Notification, {:?}", n);
                            if let Some(v) = listeners.get(n.channel()) {
                                for l in v {
                                    l.execute(n.payload());
                                }
                            }
                        }
                        Some(Ok(AsyncMessage::Notice(e))) => {
                            debug!("Handling Notice, {:?}", e);
                        }
                        Some(Ok(_)) => {
                            debug!("Handling rest");
                        }
                        Some(Err(e)) => {
                            debug!("Breaking loop due to error Error, {:?}", e);
                            break;
                        }
                        None => {
                            debug!("End of stream, breaking loop");
                            break;
                        }
                    }
                }

                drop(client);
                warn!("Restarting listener task");
            }
        });
    }
}

pub struct NewBlocks(pub State);
pub struct NewWhitelists(pub State);
pub struct NewListeners(pub State, pub JobServer);
pub struct NewActors(pub ActorCache);
pub struct NewNodes(pub NodeCache);
pub struct RmBlocks(pub State);
pub struct RmWhitelists(pub State);
pub struct RmListeners(pub State);
pub struct RmActors(pub ActorCache);
pub struct RmNodes(pub NodeCache);

impl Listener for NewBlocks {
    fn key(&self) -> &str {
        "new_blocks"
    }

    fn execute(&self, payload: &str) {
        debug!("Caching block of {}", payload);
        let state = self.0.clone();
        let payload = payload.to_owned();
        spawn(async move { state.cache_block(payload).await });
    }
}

impl Listener for NewWhitelists {
    fn key(&self) -> &str {
        "new_whitelists"
    }

    fn execute(&self, payload: &str) {
        debug!("Caching whitelist of {}", payload);
        let state = self.0.clone();
        let payload = payload.to_owned();
        spawn(async move { state.cache_whitelist(payload.to_owned()).await });
    }
}

impl Listener for NewListeners {
    fn key(&self) -> &str {
        "new_listeners"
    }

    fn execute(&self, payload: &str) {
        if let Ok(uri) = payload.parse::<Url>() {
            debug!("Caching listener {}", uri);
            let state = self.0.clone();
            let _ = self.1.queue(QueryInstance::new(uri.clone()));
            let _ = self.1.queue(QueryNodeinfo::new(uri.clone()));
            spawn(async move { state.cache_listener(uri).await });
        } else {
            warn!("Not caching listener {}, parse error", payload);
        }
    }
}

impl Listener for NewActors {
    fn key(&self) -> &str {
        "new_actors"
    }

    fn execute(&self, payload: &str) {
        if let Ok(uri) = payload.parse::<Url>() {
            debug!("Caching actor {}", uri);
            let actors = self.0.clone();
            spawn(async move { actors.cache_follower(uri).await });
        } else {
            warn!("Not caching actor {}, parse error", payload);
        }
    }
}

impl Listener for NewNodes {
    fn key(&self) -> &str {
        "new_nodes"
    }

    fn execute(&self, payload: &str) {
        if let Ok(uuid) = payload.parse::<Uuid>() {
            debug!("Caching node {}", uuid);
            let nodes = self.0.clone();
            spawn(async move { nodes.cache_by_id(uuid).await });
        } else {
            warn!("Not caching node {}, parse error", payload);
        }
    }
}

impl Listener for RmBlocks {
    fn key(&self) -> &str {
        "rm_blocks"
    }

    fn execute(&self, payload: &str) {
        debug!("Busting block cache for {}", payload);
        let state = self.0.clone();
        let payload = payload.to_owned();
        spawn(async move { state.bust_block(&payload).await });
    }
}

impl Listener for RmWhitelists {
    fn key(&self) -> &str {
        "rm_whitelists"
    }

    fn execute(&self, payload: &str) {
        debug!("Busting whitelist cache for {}", payload);
        let state = self.0.clone();
        let payload = payload.to_owned();
        spawn(async move { state.bust_whitelist(&payload).await });
    }
}

impl Listener for RmListeners {
    fn key(&self) -> &str {
        "rm_listeners"
    }

    fn execute(&self, payload: &str) {
        if let Ok(uri) = payload.parse::<Url>() {
            debug!("Busting listener cache for {}", uri);
            let state = self.0.clone();
            spawn(async move { state.bust_listener(&uri).await });
        } else {
            warn!("Not busting listener cache for {}", payload);
        }
    }
}

impl Listener for RmActors {
    fn key(&self) -> &str {
        "rm_actors"
    }

    fn execute(&self, payload: &str) {
        if let Ok(uri) = payload.parse::<Url>() {
            debug!("Busting actor cache for {}", uri);
            let actors = self.0.clone();
            spawn(async move { actors.bust_follower(&uri).await });
        } else {
            warn!("Not busting actor cache for {}", payload);
        }
    }
}

impl Listener for RmNodes {
    fn key(&self) -> &str {
        "rm_nodes"
    }

    fn execute(&self, payload: &str) {
        if let Ok(uuid) = payload.parse::<Uuid>() {
            debug!("Caching node {}", uuid);
            let nodes = self.0.clone();
            spawn(async move { nodes.bust_by_id(uuid).await });
        } else {
            warn!("Not caching node {}, parse error", payload);
        }
    }
}
