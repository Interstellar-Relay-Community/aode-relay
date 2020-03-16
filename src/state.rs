use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use bb8_postgres::tokio_postgres::{row::Row, Client};
use futures::try_join;
use log::{error, info};
use lru::LruCache;
use rand::thread_rng;
use rsa::{RSAPrivateKey, RSAPublicKey};
use rsa_pem::KeyExt;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::RwLock;
use ttl_cache::TtlCache;
use uuid::Uuid;

use crate::{apub::AcceptedActors, db_actor::Pool};

#[derive(Clone)]
pub struct State {
    pub settings: Settings,
    actor_cache: Arc<RwLock<TtlCache<XsdAnyUri, AcceptedActors>>>,
    actor_id_cache: Arc<RwLock<LruCache<XsdAnyUri, XsdAnyUri>>>,
    blocks: Arc<RwLock<HashSet<String>>>,
    whitelists: Arc<RwLock<HashSet<String>>>,
    listeners: Arc<RwLock<HashSet<XsdAnyUri>>>,
}

#[derive(Clone)]
pub struct Settings {
    pub use_https: bool,
    pub whitelist_enabled: bool,
    pub hostname: String,
    pub public_key: RSAPublicKey,
    private_key: RSAPrivateKey,
}

pub enum UrlKind {
    Activity,
    Actor,
    Followers,
    Following,
    Inbox,
    MainKey,
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("No host present in URI")]
pub struct HostError;

#[derive(Clone, Debug, thiserror::Error)]
#[error("Error generating RSA key")]
pub struct RsaError;

impl Settings {
    async fn hydrate(
        client: &Client,
        use_https: bool,
        whitelist_enabled: bool,
        hostname: String,
    ) -> Result<Self, Error> {
        info!("SELECT value FROM settings WHERE key = 'private_key'");
        let rows = client
            .query("SELECT value FROM settings WHERE key = 'private_key'", &[])
            .await?;

        let private_key = if let Some(row) = rows.into_iter().next() {
            let key_str: String = row.get(0);
            KeyExt::from_pem_pkcs8(&key_str)?
        } else {
            info!("Generating new keys");
            let mut rng = thread_rng();
            let key = RSAPrivateKey::new(&mut rng, 4096).map_err(|e| {
                error!("Error generating RSA key, {}", e);
                RsaError
            })?;
            let pem_pkcs8 = key.to_pem_pkcs8()?;

            info!("INSERT INTO settings (key, value, created_at) VALUES ('private_key', $1::TEXT, 'now');");
            client.execute("INSERT INTO settings (key, value, created_at) VALUES ('private_key', $1::TEXT, 'now');", &[&pem_pkcs8]).await?;
            key
        };

        let public_key = private_key.to_public_key();

        Ok(Settings {
            use_https,
            whitelist_enabled,
            hostname,
            private_key,
            public_key,
        })
    }

    fn generate_url(&self, kind: UrlKind) -> String {
        let scheme = if self.use_https { "https" } else { "http" };

        match kind {
            UrlKind::Activity => {
                format!("{}://{}/activity/{}", scheme, self.hostname, Uuid::new_v4())
            }
            UrlKind::Actor => format!("{}://{}/actor", scheme, self.hostname),
            UrlKind::Followers => format!("{}://{}/followers", scheme, self.hostname),
            UrlKind::Following => format!("{}://{}/following", scheme, self.hostname),
            UrlKind::Inbox => format!("{}://{}/inbox", scheme, self.hostname),
            UrlKind::MainKey => format!("{}://{}/actor#main-key", scheme, self.hostname),
        }
    }

    fn generate_resource(&self) -> String {
        format!("relay@{}", self.hostname)
    }

    fn sign(&self, bytes: &[u8]) -> Result<String, crate::error::MyError> {
        use rsa::{hash::Hashes, padding::PaddingScheme};
        let bytes =
            self.private_key
                .sign(PaddingScheme::PKCS1v15, Some(&Hashes::SHA2_256), bytes)?;
        Ok(base64::encode_config(bytes, base64::URL_SAFE))
    }
}

impl State {
    pub fn generate_url(&self, kind: UrlKind) -> String {
        self.settings.generate_url(kind)
    }

    pub fn generate_resource(&self) -> String {
        self.settings.generate_resource()
    }

    pub fn sign(&self, bytes: &[u8]) -> Result<String, crate::error::MyError> {
        self.settings.sign(bytes)
    }

    pub async fn remove_listener(&self, client: &Client, inbox: &XsdAnyUri) -> Result<(), Error> {
        let hs = self.listeners.clone();

        info!("DELETE FROM listeners WHERE actor_id = {};", inbox.as_str());
        client
            .execute(
                "DELETE FROM listeners WHERE actor_id = $1::TEXT;",
                &[&inbox.as_str()],
            )
            .await?;

        let mut write_guard = hs.write().await;
        write_guard.remove(inbox);

        Ok(())
    }

    pub async fn listeners_without(&self, inbox: &XsdAnyUri, domain: &str) -> Vec<XsdAnyUri> {
        let hs = self.listeners.clone();

        let read_guard = hs.read().await;

        read_guard
            .iter()
            .filter_map(|listener| {
                if let Some(host) = listener.as_url().host() {
                    if listener != inbox && host.to_string() != domain {
                        return Some(listener.clone());
                    }
                }

                None
            })
            .collect()
    }

    pub async fn is_whitelisted(&self, actor_id: &XsdAnyUri) -> bool {
        if !self.settings.whitelist_enabled {
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

        info!(
            "INSERT INTO blocks (domain_name, created_at) VALUES ($1::TEXT, 'now'); [{}]",
            host.to_string()
        );
        client
            .execute(
                "INSERT INTO blocks (domain_name, created_at) VALUES ($1::TEXT, 'now');",
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

        info!(
            "INSERT INTO whitelists (domain_name, created_at) VALUES ($1::TEXT, 'now'); [{}]",
            host.to_string()
        );
        client
            .execute(
                "INSERT INTO whitelists (domain_name, created_at) VALUES ($1::TEXT, 'now');",
                &[&host.to_string()],
            )
            .await?;

        let mut write_guard = whitelists.write().await;
        write_guard.insert(host.to_string());

        Ok(())
    }

    pub async fn add_listener(&self, client: &Client, listener: XsdAnyUri) -> Result<(), Error> {
        let listeners = self.listeners.clone();

        info!(
            "INSERT INTO listeners (actor_id, created_at) VALUES ($1::TEXT, 'now'); [{}]",
            listener.as_str(),
        );
        client
            .execute(
                "INSERT INTO listeners (actor_id, created_at) VALUES ($1::TEXT, 'now');",
                &[&listener.as_str()],
            )
            .await?;

        let mut write_guard = listeners.write().await;
        write_guard.insert(listener);

        Ok(())
    }

    pub async fn hydrate(
        use_https: bool,
        whitelist_enabled: bool,
        hostname: String,
        pool: Pool,
    ) -> Result<Self, Error> {
        let pool1 = pool.clone();
        let pool2 = pool.clone();
        let pool3 = pool.clone();

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

        let f4 = async move {
            let conn = pool3.get().await?;

            Settings::hydrate(&conn, use_https, whitelist_enabled, hostname).await
        };

        let (blocks, whitelists, listeners, settings) = try_join!(f1, f2, f3, f4)?;

        Ok(State {
            settings,
            actor_cache: Arc::new(RwLock::new(TtlCache::new(1024 * 8))),
            actor_id_cache: Arc::new(RwLock::new(LruCache::new(1024 * 8))),
            blocks: Arc::new(RwLock::new(blocks)),
            whitelists: Arc::new(RwLock::new(whitelists)),
            listeners: Arc::new(RwLock::new(listeners)),
        })
    }
}

pub async fn hydrate_blocks(client: &Client) -> Result<HashSet<String>, Error> {
    info!("SELECT domain_name FROM blocks");
    let rows = client.query("SELECT domain_name FROM blocks", &[]).await?;

    parse_rows(rows)
}

pub async fn hydrate_whitelists(client: &Client) -> Result<HashSet<String>, Error> {
    info!("SELECT domain_name FROM whitelists");
    let rows = client
        .query("SELECT domain_name FROM whitelists", &[])
        .await?;

    parse_rows(rows)
}

pub async fn hydrate_listeners(client: &Client) -> Result<HashSet<XsdAnyUri>, Error> {
    info!("SELECT actor_id FROM listeners");
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
