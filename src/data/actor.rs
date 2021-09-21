use crate::{
    apub::AcceptedActors,
    db::{Actor, Db},
    error::{Error, ErrorKind},
    requests::Requests,
};
use activitystreams::{prelude::*, url::Url};
use std::time::{Duration, SystemTime};

const REFETCH_DURATION: Duration = Duration::from_secs(60 * 30);

#[derive(Debug)]
pub enum MaybeCached<T> {
    Cached(T),
    Fetched(T),
}

impl<T> MaybeCached<T> {
    pub(crate) fn is_cached(&self) -> bool {
        match self {
            MaybeCached::Cached(_) => true,
            _ => false,
        }
    }

    pub(crate) fn into_inner(self) -> T {
        match self {
            MaybeCached::Cached(t) | MaybeCached::Fetched(t) => t,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActorCache {
    db: Db,
}

impl ActorCache {
    pub(crate) fn new(db: Db) -> Self {
        ActorCache { db }
    }

    #[tracing::instrument(name = "Get Actor", fields(id = id.to_string().as_str(), requests))]
    pub(crate) async fn get(
        &self,
        id: &Url,
        requests: &Requests,
    ) -> Result<MaybeCached<Actor>, Error> {
        if let Some(actor) = self.db.actor(id.clone()).await? {
            if actor.saved_at + REFETCH_DURATION > SystemTime::now() {
                return Ok(MaybeCached::Cached(actor));
            }
        }

        self.get_no_cache(id, requests)
            .await
            .map(MaybeCached::Fetched)
    }

    #[tracing::instrument(name = "Add Connection")]
    pub(crate) async fn add_connection(&self, actor: Actor) -> Result<(), Error> {
        self.db.add_connection(actor.id.clone()).await?;
        self.db.save_actor(actor).await
    }

    #[tracing::instrument(name = "Remove Connection")]
    pub(crate) async fn remove_connection(&self, actor: &Actor) -> Result<(), Error> {
        self.db.remove_connection(actor.id.clone()).await
    }

    #[tracing::instrument(name = "Fetch remote actor", fields(id = id.to_string().as_str(), requests))]
    pub(crate) async fn get_no_cache(&self, id: &Url, requests: &Requests) -> Result<Actor, Error> {
        let accepted_actor = requests.fetch::<AcceptedActors>(id.as_str()).await?;

        let input_domain = id.domain().ok_or(ErrorKind::MissingDomain)?;
        let accepted_actor_id = accepted_actor
            .id(&input_domain)?
            .ok_or(ErrorKind::MissingId)?;

        let inbox = get_inbox(&accepted_actor)?.clone();

        let actor = Actor {
            id: accepted_actor_id.clone().into(),
            public_key: accepted_actor.ext_one.public_key.public_key_pem,
            public_key_id: accepted_actor.ext_one.public_key.id,
            inbox: inbox.into(),
            saved_at: SystemTime::now(),
        };

        self.db.save_actor(actor.clone()).await?;

        Ok(actor)
    }
}

fn get_inbox(actor: &AcceptedActors) -> Result<&Url, Error> {
    Ok(actor
        .endpoints()?
        .and_then(|e| e.shared_inbox)
        .unwrap_or(actor.inbox()?))
}
