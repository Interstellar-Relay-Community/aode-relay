use crate::{apub::AcceptedActors, db::Db, error::MyError, requests::Requests};
use activitystreams::primitives::XsdAnyUri;
use log::error;
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use ttl_cache::TtlCache;
use uuid::Uuid;

const REFETCH_DURATION: Duration = Duration::from_secs(60 * 30);

#[derive(Clone)]
pub struct ActorCache {
    db: Db,
    cache: Arc<RwLock<TtlCache<XsdAnyUri, Actor>>>,
    following: Arc<RwLock<HashSet<XsdAnyUri>>>,
}

impl ActorCache {
    pub fn new(db: Db) -> Self {
        let cache = ActorCache {
            db,
            cache: Arc::new(RwLock::new(TtlCache::new(1024 * 8))),
            following: Arc::new(RwLock::new(HashSet::new())),
        };

        cache.spawn_rehydrate();

        cache
    }

    pub async fn is_following(&self, id: &XsdAnyUri) -> bool {
        self.following.read().await.contains(id)
    }

    pub async fn get(&self, id: &XsdAnyUri, requests: &Requests) -> Result<Actor, MyError> {
        if let Some(actor) = self.cache.read().await.get(id) {
            return Ok(actor.clone());
        }

        if let Some(actor) = self.lookup(id).await? {
            self.cache
                .write()
                .await
                .insert(id.clone(), actor.clone(), REFETCH_DURATION);
            return Ok(actor);
        }

        let accepted_actor = requests.fetch::<AcceptedActors>(id.as_str()).await?;

        let actor_host = accepted_actor.id.as_url().host();
        let inbox_host = accepted_actor.inbox().as_url().host();

        if actor_host != inbox_host {
            let actor_host = actor_host.map(|h| h.to_string()).unwrap_or(String::new());
            let inbox_host = inbox_host.map(|h| h.to_string()).unwrap_or(String::new());

            return Err(MyError::HostMismatch(actor_host, inbox_host));
        }

        let inbox = accepted_actor.inbox().clone();

        let actor = Actor {
            id: accepted_actor.id,
            public_key: accepted_actor.public_key.public_key_pem,
            public_key_id: accepted_actor.public_key.id,
            inbox,
        };

        self.cache
            .write()
            .await
            .insert(id.clone(), actor.clone(), REFETCH_DURATION);

        self.update(id, &actor.public_key, &actor.public_key_id)
            .await?;

        Ok(actor)
    }

    pub async fn follower(&self, actor: &Actor) -> Result<(), MyError> {
        self.save(actor.clone()).await
    }

    pub async fn cache_follower(&self, id: XsdAnyUri) {
        self.following.write().await.insert(id);
    }

    pub async fn bust_follower(&self, id: &XsdAnyUri) {
        self.following.write().await.remove(id);
    }

    pub async fn unfollower(&self, actor: &Actor) -> Result<Option<Uuid>, MyError> {
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "DELETE FROM actors
                 WHERE actor_id = $1::TEXT
                 RETURNING listener_id;",
                &[&actor.id.as_str()],
            )
            .await?;

        let row = if let Some(row) = row_opt {
            row
        } else {
            return Ok(None);
        };

        let listener_id: Uuid = row.try_get(0)?;

        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT FROM actors
                WHERE listener_id = $1::UUID;",
                &[&listener_id],
            )
            .await?;

        if row_opt.is_none() {
            return Ok(Some(listener_id));
        }

        Ok(None)
    }

    async fn lookup(&self, id: &XsdAnyUri) -> Result<Option<Actor>, MyError> {
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT listeners.actor_id, actors.public_key, actors.public_key_id
                 FROM listeners
                 INNER JOIN actors ON actors.listener_id = listeners.id
                 WHERE
                    actors.actor_id = $1::TEXT
                 AND
                    actors.updated_at + INTERVAL '120 seconds' < NOW()
                 LIMIT 1;",
                &[&id.as_str()],
            )
            .await?;

        let row = if let Some(row) = row_opt {
            row
        } else {
            return Ok(None);
        };

        let inbox: String = row.try_get(0)?;
        let public_key_id: String = row.try_get(2)?;

        Ok(Some(Actor {
            id: id.clone(),
            inbox: inbox.parse()?,
            public_key: row.try_get(1)?,
            public_key_id: public_key_id.parse()?,
        }))
    }

    async fn save(&self, actor: Actor) -> Result<(), MyError> {
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT id FROM listeners WHERE actor_id = $1::TEXT LIMIT 1;",
                &[&actor.inbox.as_str()],
            )
            .await?;

        let row = if let Some(row) = row_opt {
            row
        } else {
            return Err(MyError::NotSubscribed(actor.id.as_str().to_owned()));
        };

        let listener_id: Uuid = row.try_get(0)?;

        self.db
            .pool()
            .get()
            .await?
            .execute(
                "INSERT INTO actors (
                    actor_id,
                    public_key,
                    public_key_id,
                    listener_id,
                    created_at,
                    updated_at
                 ) VALUES (
                    $1::TEXT,
                    $2::TEXT,
                    $3::TEXT,
                    $4::UUID,
                    'now',
                    'now'
                 ) ON CONFLICT (actor_id)
                 DO UPDATE SET public_key = $2::TEXT;",
                &[
                    &actor.id.as_str(),
                    &actor.public_key,
                    &actor.public_key_id.as_str(),
                    &listener_id,
                ],
            )
            .await?;
        Ok(())
    }

    async fn update(
        &self,
        id: &XsdAnyUri,
        public_key: &str,
        public_key_id: &XsdAnyUri,
    ) -> Result<(), MyError> {
        self.db
            .pool()
            .get()
            .await?
            .execute(
                "UPDATE actors
                 SET public_key = $2::TEXT, public_key_id = $3::TEXT
                 WHERE actor_id = $1::TEXT;",
                &[&id.as_str(), &public_key, &public_key_id.as_str()],
            )
            .await?;

        Ok(())
    }

    fn spawn_rehydrate(&self) {
        use actix::clock::{interval_at, Instant};

        let this = self.clone();
        actix::spawn(async move {
            let mut interval = interval_at(Instant::now(), Duration::from_secs(60 * 10));

            loop {
                if let Err(e) = this.rehydrate().await {
                    error!("Error rehydrating follows, {}", e);
                }

                interval.tick().await;
            }
        });
    }

    async fn rehydrate(&self) -> Result<(), MyError> {
        let rows = self
            .db
            .pool()
            .get()
            .await?
            .query("SELECT actor_id FROM actors;", &[])
            .await?;

        let actor_ids = rows
            .into_iter()
            .filter_map(|row| match row.try_get(0) {
                Ok(s) => {
                    let s: String = s;
                    match s.parse() {
                        Ok(s) => Some(s),
                        Err(e) => {
                            error!("Error parsing actor id, {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    error!("Error getting actor id from row, {}", e);
                    None
                }
            })
            .collect();

        let mut write_guard = self.following.write().await;
        *write_guard = actor_ids;
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Actor {
    pub id: XsdAnyUri,
    pub public_key: String,
    pub public_key_id: XsdAnyUri,
    pub inbox: XsdAnyUri,
}
