use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use bb8_postgres::tokio_postgres::{row::Row, Client};
use futures::try_join;
use lru::LruCache;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::RwLock;
use ttl_cache::TtlCache;

use crate::{apub::AcceptedActors, db_actor::Pool};

#[derive(Clone)]
pub struct State {
    whitelist_enabled: bool,
    actor_cache: Arc<RwLock<TtlCache<XsdAnyUri, AcceptedActors>>>,
    actor_id_cache: Arc<RwLock<LruCache<XsdAnyUri, XsdAnyUri>>>,
    blocks: Arc<RwLock<HashSet<String>>>,
    whitelists: Arc<RwLock<HashSet<String>>>,
    listeners: Arc<RwLock<HashSet<XsdAnyUri>>>,
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("No host present in URI")]
pub struct HostError;

impl State {
    pub async fn is_whitelisted(&self, actor_id: &XsdAnyUri) -> bool {
        if !self.whitelist_enabled {
            return true;
        }

        let hs = self.whitelists.clone();

        if let Some(host) = actor_id.as_url().host() {
            let read_guard = hs.read().await;
            return read_guard.contains(&host.to_string());
        }

        false
    }

    pub async fn is_blocked(&self, actor_id: &XsdAnyUri) -> bool {
        let hs = self.blocks.clone();

        if let Some(host) = actor_id.as_url().host() {
            let read_guard = hs.read().await;
            return read_guard.contains(&host.to_string());
        }

        true
    }

    pub async fn is_listener(&self, actor_id: &XsdAnyUri) -> bool {
        let hs = self.listeners.clone();

        let read_guard = hs.read().await;
        read_guard.contains(actor_id)
    }

    pub async fn get_actor(&self, actor_id: &XsdAnyUri) -> Option<AcceptedActors> {
        let cache = self.actor_cache.clone();

        let read_guard = cache.read().await;
        read_guard.get(actor_id).cloned()
    }

    pub async fn cache_actor(&self, actor_id: XsdAnyUri, actor: AcceptedActors) {
        let cache = self.actor_cache.clone();

        let mut write_guard = cache.write().await;
        write_guard.insert(actor_id, actor, std::time::Duration::from_secs(3600));
    }

    pub async fn is_cached(&self, object_id: &XsdAnyUri) -> bool {
        let cache = self.actor_id_cache.clone();

        let read_guard = cache.read().await;
        read_guard.contains(object_id)
    }

    pub async fn cache(&self, object_id: XsdAnyUri, actor_id: XsdAnyUri) {
        let cache = self.actor_id_cache.clone();

        let mut write_guard = cache.write().await;
        write_guard.put(object_id, actor_id);
    }

    pub async fn add_block(&self, client: &Client, block: XsdAnyUri) -> Result<(), Error> {
        let blocks = self.blocks.clone();

        let host = if let Some(host) = block.as_url().host() {
            host
        } else {
            return Err(HostError.into());
        };

        client
            .execute(
                "INSERT INTO blocks (actor_id, created_at) VALUES ($1::TEXT, now);",
                &[&host.to_string()],
            )
            .await?;

        let mut write_guard = blocks.write().await;
        write_guard.insert(host.to_string());

        Ok(())
    }

    pub async fn add_whitelist(&self, client: &Client, whitelist: XsdAnyUri) -> Result<(), Error> {
        let whitelists = self.whitelists.clone();

        let host = if let Some(host) = whitelist.as_url().host() {
            host
        } else {
            return Err(HostError.into());
        };

        client
            .execute(
                "INSERT INTO whitelists (actor_id, created_at) VALUES ($1::TEXT, now);",
                &[&host.to_string()],
            )
            .await?;

        let mut write_guard = whitelists.write().await;
        write_guard.insert(host.to_string());

        Ok(())
    }

    pub async fn add_listener(&self, client: &Client, listener: XsdAnyUri) -> Result<(), Error> {
        let listeners = self.listeners.clone();

        client
            .execute(
                "INSERT INTO listeners (actor_id, created_at) VALUES ($1::TEXT, now);",
                &[&listener.as_str()],
            )
            .await?;

        let mut write_guard = listeners.write().await;
        write_guard.insert(listener);

        Ok(())
    }

    pub async fn hydrate(whitelist_enabled: bool, pool: Pool) -> Result<Self, Error> {
        let pool1 = pool.clone();
        let pool2 = pool.clone();

        let f1 = async move {
            let conn = pool.get().await?;

            hydrate_blocks(&conn).await
        };

        let f2 = async move {
            let conn = pool1.get().await?;

            hydrate_whitelists(&conn).await
        };

        let f3 = async move {
            let conn = pool2.get().await?;

            hydrate_listeners(&conn).await
        };

        let (blocks, whitelists, listeners) = try_join!(f1, f2, f3)?;

        Ok(State {
            whitelist_enabled,
            actor_cache: Arc::new(RwLock::new(TtlCache::new(1024 * 8))),
            actor_id_cache: Arc::new(RwLock::new(LruCache::new(1024 * 8))),
            blocks: Arc::new(RwLock::new(blocks)),
            whitelists: Arc::new(RwLock::new(whitelists)),
            listeners: Arc::new(RwLock::new(listeners)),
        })
    }
}

pub async fn hydrate_blocks(client: &Client) -> Result<HashSet<String>, Error> {
    let rows = client.query("SELECT domain_name FROM blocks", &[]).await?;

    parse_rows(rows)
}

pub async fn hydrate_whitelists(client: &Client) -> Result<HashSet<String>, Error> {
    let rows = client
        .query("SELECT domain_name FROM whitelists", &[])
        .await?;

    parse_rows(rows)
}

pub async fn hydrate_listeners(client: &Client) -> Result<HashSet<XsdAnyUri>, Error> {
    let rows = client.query("SELECT actor_id FROM listeners", &[]).await?;

    parse_rows(rows)
}

pub fn parse_rows<T>(rows: Vec<Row>) -> Result<HashSet<T>, Error>
where
    T: std::str::FromStr + Eq + std::hash::Hash,
{
    let hs = rows
        .into_iter()
        .filter_map(move |row| {
            let s: String = row.try_get(0).ok()?;
            s.parse().ok()
        })
        .collect();

    Ok(hs)
}
