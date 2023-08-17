use crate::{
    data::NodeCache,
    db::Db,
    error::Error,
    requests::{Breakers, Requests},
    spawner::Spawner,
};
use activitystreams::iri_string::types::IriString;
use actix_web::web;
use lru::LruCache;
use rand::thread_rng;
use reqwest_middleware::ClientWithMiddleware;
use rsa::{RsaPrivateKey, RsaPublicKey};
use std::sync::{Arc, RwLock};

use super::LastOnline;

#[derive(Clone)]
pub struct State {
    pub(crate) requests: Requests,
    pub(crate) public_key: RsaPublicKey,
    object_cache: Arc<RwLock<LruCache<IriString, IriString>>>,
    pub(crate) node_cache: NodeCache,
    breakers: Breakers,
    pub(crate) last_online: Arc<LastOnline>,
    pub(crate) db: Db,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("node_cache", &self.node_cache)
            .field("breakers", &self.breakers)
            .field("db", &self.db)
            .finish()
    }
}

impl State {
    #[tracing::instrument(
        level = "debug",
        name = "Get inboxes for other domains",
        skip_all,
        fields(
            existing_inbox = existing_inbox.to_string().as_str(),
            authority
        )
    )]
    pub(crate) async fn inboxes_without(
        &self,
        existing_inbox: &IriString,
        authority: &str,
    ) -> Result<Vec<IriString>, Error> {
        Ok(self
            .db
            .inboxes()
            .await?
            .iter()
            .filter_map(|inbox| {
                if let Some(authority_str) = inbox.authority_str() {
                    if inbox != existing_inbox && authority_str != authority {
                        return Some(inbox.clone());
                    }
                }

                None
            })
            .collect())
    }

    pub(crate) fn is_cached(&self, object_id: &IriString) -> bool {
        self.object_cache.read().unwrap().contains(object_id)
    }

    pub(crate) fn cache(&self, object_id: IriString, actor_id: IriString) {
        self.object_cache.write().unwrap().put(object_id, actor_id);
    }

    pub(crate) fn is_connected(&self, iri: &IriString) -> bool {
        self.breakers.should_try(iri)
    }

    #[tracing::instrument(level = "debug", name = "Building state", skip_all)]
    pub(crate) async fn build(
        db: Db,
        key_id: String,
        spawner: Spawner,
        client: ClientWithMiddleware,
    ) -> Result<Self, Error> {
        let private_key = if let Ok(Some(key)) = db.private_key().await {
            tracing::debug!("Using existing key");
            key
        } else {
            tracing::info!("Generating new keys");
            let key = web::block(move || {
                let mut rng = thread_rng();
                RsaPrivateKey::new(&mut rng, 4096)
            })
            .await??;

            db.update_private_key(&key).await?;

            key
        };

        let public_key = private_key.to_public_key();

        let breakers = Breakers::default();
        let last_online = Arc::new(LastOnline::empty());

        let requests = Requests::new(
            key_id,
            private_key,
            breakers.clone(),
            last_online.clone(),
            spawner,
            client,
        );

        let state = State {
            requests,
            public_key,
            object_cache: Arc::new(RwLock::new(LruCache::new(
                (1024 * 8).try_into().expect("nonzero"),
            ))),
            node_cache: NodeCache::new(db.clone()),
            breakers,
            db,
            last_online,
        };

        Ok(state)
    }
}
