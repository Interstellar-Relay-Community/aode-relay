use crate::error::MyError;
use activitystreams::primitives::XsdAnyUri;
use bb8_postgres::{
    bb8,
    tokio_postgres::{
        error::{Error, SqlState},
        row::Row,
        Client, Config, NoTls,
    },
    PostgresConnectionManager,
};
use log::{info, warn};
use rsa::RSAPrivateKey;
use rsa_pem::KeyExt;
use std::{collections::HashSet, convert::TryInto};

pub type Pool = bb8::Pool<PostgresConnectionManager<NoTls>>;

#[derive(Clone)]
pub struct Db {
    pool: Pool,
}

impl Db {
    pub async fn build(config: &crate::config::Config) -> Result<Self, MyError> {
        let cpus: u32 = num_cpus::get().try_into()?;
        let max_conns = cpus * config.connections_per_core();

        let config: Config = config.database_url().parse()?;
        let manager = PostgresConnectionManager::new(config, NoTls);

        let pool = bb8::Pool::builder()
            .max_size(max_conns)
            .build(manager)
            .await?;

        Ok(Db { pool })
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub async fn remove_listener(&self, inbox: XsdAnyUri) -> Result<(), MyError> {
        let conn = self.pool.get().await?;

        remove_listener(&conn, &inbox).await?;
        Ok(())
    }

    pub async fn add_listener(&self, inbox: XsdAnyUri) -> Result<(), MyError> {
        let conn = self.pool.get().await?;

        add_listener(&conn, &inbox).await?;
        Ok(())
    }

    pub async fn add_blocks(&self, domains: &[String]) -> Result<(), MyError> {
        let conn = self.pool.get().await?;
        for domain in domains {
            match add_block(&conn, domain.as_str()).await {
                Err(e) if e.code() != Some(&SqlState::UNIQUE_VIOLATION) => {
                    Err(e)?;
                }
                _ => (),
            };
        }
        Ok(())
    }

    pub async fn remove_blocks(&self, domains: &[String]) -> Result<(), MyError> {
        let conn = self.pool.get().await?;
        for domain in domains {
            remove_block(&conn, domain.as_str()).await?
        }
        Ok(())
    }

    pub async fn add_whitelists(&self, domains: &[String]) -> Result<(), MyError> {
        let conn = self.pool.get().await?;
        for domain in domains {
            match add_whitelist(&conn, domain.as_str()).await {
                Err(e) if e.code() != Some(&SqlState::UNIQUE_VIOLATION) => {
                    Err(e)?;
                }
                _ => (),
            };
        }
        Ok(())
    }

    pub async fn remove_whitelists(&self, domains: &[String]) -> Result<(), MyError> {
        let conn = self.pool.get().await?;
        for domain in domains {
            remove_whitelist(&conn, domain.as_str()).await?
        }
        Ok(())
    }

    pub async fn hydrate_blocks(&self) -> Result<HashSet<String>, MyError> {
        let conn = self.pool.get().await?;

        Ok(hydrate_blocks(&conn).await?)
    }

    pub async fn hydrate_whitelists(&self) -> Result<HashSet<String>, MyError> {
        let conn = self.pool.get().await?;

        Ok(hydrate_whitelists(&conn).await?)
    }

    pub async fn hydrate_listeners(&self) -> Result<HashSet<XsdAnyUri>, MyError> {
        let conn = self.pool.get().await?;

        Ok(hydrate_listeners(&conn).await?)
    }

    pub async fn hydrate_private_key(&self) -> Result<Option<RSAPrivateKey>, MyError> {
        let conn = self.pool.get().await?;

        Ok(hydrate_private_key(&conn).await?)
    }

    pub async fn update_private_key(&self, private_key: &RSAPrivateKey) -> Result<(), MyError> {
        let conn = self.pool.get().await?;

        Ok(update_private_key(&conn, private_key).await?)
    }
}

pub async fn listen(client: &Client) -> Result<(), Error> {
    info!("LISTEN new_blocks, new_whitelists, new_listeners, new_actors, rm_blocks, rm_whitelists, rm_listeners, rm_actors");
    client
        .batch_execute(
            "LISTEN new_blocks;
             LISTEN new_whitelists;
             LISTEN new_listeners;
             LISTEN new_actors;
             LISTEN rm_blocks;
             LISTEN rm_whitelists;
             LISTEN rm_listeners;
             LISTEN rm_actors;",
        )
        .await?;

    Ok(())
}

async fn hydrate_private_key(client: &Client) -> Result<Option<RSAPrivateKey>, MyError> {
    info!("SELECT value FROM settings WHERE key = 'private_key'");
    let rows = client
        .query("SELECT value FROM settings WHERE key = 'private_key'", &[])
        .await?;

    if let Some(row) = rows.into_iter().next() {
        let key_str: String = row.get(0);
        return Ok(Some(KeyExt::from_pem_pkcs8(&key_str)?));
    }

    Ok(None)
}

async fn update_private_key(client: &Client, key: &RSAPrivateKey) -> Result<(), MyError> {
    let pem_pkcs8 = key.to_pem_pkcs8()?;

    info!("INSERT INTO settings (key, value, created_at) VALUES ('private_key', $1::TEXT, 'now');");
    client.execute("INSERT INTO settings (key, value, created_at) VALUES ('private_key', $1::TEXT, 'now');", &[&pem_pkcs8]).await?;
    Ok(())
}

async fn add_block(client: &Client, domain: &str) -> Result<(), Error> {
    info!(
        "INSERT INTO blocks (domain_name, created_at) VALUES ($1::TEXT, 'now'); [{}]",
        domain,
    );
    client
        .execute(
            "INSERT INTO blocks (domain_name, created_at) VALUES ($1::TEXT, 'now');",
            &[&domain],
        )
        .await?;

    Ok(())
}

async fn remove_block(client: &Client, domain: &str) -> Result<(), Error> {
    info!(
        "DELETE FROM blocks WHERE domain_name = $1::TEXT; [{}]",
        domain,
    );
    client
        .execute(
            "DELETE FROM blocks WHERE domain_name = $1::TEXT;",
            &[&domain],
        )
        .await?;

    Ok(())
}

async fn add_whitelist(client: &Client, domain: &str) -> Result<(), Error> {
    info!(
        "INSERT INTO whitelists (domain_name, created_at) VALUES ($1::TEXT, 'now'); [{}]",
        domain,
    );
    client
        .execute(
            "INSERT INTO whitelists (domain_name, created_at) VALUES ($1::TEXT, 'now');",
            &[&domain],
        )
        .await?;

    Ok(())
}

async fn remove_whitelist(client: &Client, domain: &str) -> Result<(), Error> {
    info!(
        "DELETE FROM whitelists WHERE domain_name = $1::TEXT; [{}]",
        domain,
    );
    client
        .execute(
            "DELETE FROM whitelists WHERE domain_name = $1::TEXT;",
            &[&domain],
        )
        .await?;

    Ok(())
}

async fn remove_listener(client: &Client, listener: &XsdAnyUri) -> Result<(), Error> {
    info!(
        "DELETE FROM listeners WHERE actor_id = {};",
        listener.as_str()
    );
    client
        .execute(
            "DELETE FROM listeners WHERE actor_id = $1::TEXT;",
            &[&listener.as_str()],
        )
        .await?;

    Ok(())
}

async fn add_listener(client: &Client, listener: &XsdAnyUri) -> Result<(), Error> {
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

    Ok(())
}

async fn hydrate_blocks(client: &Client) -> Result<HashSet<String>, Error> {
    info!("SELECT domain_name FROM blocks");
    let rows = client.query("SELECT domain_name FROM blocks", &[]).await?;

    parse_rows(rows)
}

async fn hydrate_whitelists(client: &Client) -> Result<HashSet<String>, Error> {
    info!("SELECT domain_name FROM whitelists");
    let rows = client
        .query("SELECT domain_name FROM whitelists", &[])
        .await?;

    parse_rows(rows)
}

async fn hydrate_listeners(client: &Client) -> Result<HashSet<XsdAnyUri>, Error> {
    info!("SELECT actor_id FROM listeners");
    let rows = client.query("SELECT actor_id FROM listeners", &[]).await?;

    parse_rows(rows)
}

fn parse_rows<T, E>(rows: Vec<Row>) -> Result<HashSet<T>, Error>
where
    T: std::str::FromStr<Err = E> + Eq + std::hash::Hash,
    E: std::fmt::Display,
{
    let hs = rows
        .into_iter()
        .filter_map(move |row| match row.try_get::<_, String>(0) {
            Ok(s) => match s.parse() {
                Ok(t) => Some(t),
                Err(e) => {
                    warn!("Couln't parse row, '{}', {}", s, e);
                    None
                }
            },
            Err(e) => {
                warn!("Couldn't get column, {}", e);
                None
            }
        })
        .collect();

    Ok(hs)
}
