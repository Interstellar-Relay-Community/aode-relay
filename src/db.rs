use crate::error::MyError;
use activitystreams::primitives::XsdAnyUri;
use deadpool_postgres::{Manager, Pool};
use log::{info, warn};
use rsa::RSAPrivateKey;
use rsa_pem::KeyExt;
use std::collections::HashSet;
use tokio_postgres::{
    error::{Error, SqlState},
    row::Row,
    Client, Config, NoTls,
};

#[derive(Clone)]
pub struct Db {
    pool: Pool,
}

impl Db {
    pub fn build(config: &crate::config::Config) -> Result<Self, MyError> {
        let max_conns = config.max_connections();
        let config: Config = config.database_url().parse()?;

        let manager = Manager::new(config, NoTls);

        Ok(Db {
            pool: Pool::new(manager, max_conns),
        })
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub async fn remove_listener(&self, inbox: XsdAnyUri) -> Result<(), MyError> {
        info!("DELETE FROM listeners WHERE actor_id = {};", inbox.as_str());
        self.pool
            .get()
            .await?
            .execute(
                "DELETE FROM listeners WHERE actor_id = $1::TEXT;",
                &[&inbox.as_str()],
            )
            .await?;

        Ok(())
    }

    pub async fn add_listener(&self, inbox: XsdAnyUri) -> Result<(), MyError> {
        info!(
            "INSERT INTO listeners (actor_id, created_at) VALUES ($1::TEXT, 'now'); [{}]",
            inbox.as_str(),
        );
        self.pool
            .get()
            .await?
            .execute(
                "INSERT INTO listeners (actor_id, created_at) VALUES ($1::TEXT, 'now');",
                &[&inbox.as_str()],
            )
            .await?;

        Ok(())
    }

    pub async fn add_blocks(&self, domains: &[String]) -> Result<(), MyError> {
        let conn = self.pool.get().await?;
        for domain in domains {
            match add_block(&conn, domain.as_str()).await {
                Err(e) if e.code() != Some(&SqlState::UNIQUE_VIOLATION) => {
                    return Err(e.into());
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
                    return Err(e.into());
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
        info!("SELECT domain_name FROM blocks");
        let rows = self
            .pool
            .get()
            .await?
            .query("SELECT domain_name FROM blocks", &[])
            .await?;

        parse_rows(rows)
    }

    pub async fn hydrate_whitelists(&self) -> Result<HashSet<String>, MyError> {
        info!("SELECT domain_name FROM whitelists");
        let rows = self
            .pool
            .get()
            .await?
            .query("SELECT domain_name FROM whitelists", &[])
            .await?;

        parse_rows(rows)
    }

    pub async fn hydrate_listeners(&self) -> Result<HashSet<XsdAnyUri>, MyError> {
        info!("SELECT actor_id FROM listeners");
        let rows = self
            .pool
            .get()
            .await?
            .query("SELECT actor_id FROM listeners", &[])
            .await?;

        parse_rows(rows)
    }

    pub async fn hydrate_private_key(&self) -> Result<Option<RSAPrivateKey>, MyError> {
        info!("SELECT value FROM settings WHERE key = 'private_key'");
        let rows = self
            .pool
            .get()
            .await?
            .query("SELECT value FROM settings WHERE key = 'private_key'", &[])
            .await?;

        if let Some(row) = rows.into_iter().next() {
            let key_str: String = row.get(0);
            // precomputation happens when constructing a private key, so it should be on the
            // threadpool
            let key = actix_web::web::block(move || KeyExt::from_pem_pkcs8(&key_str)).await?;

            return Ok(Some(key));
        }

        Ok(None)
    }

    pub async fn update_private_key(&self, private_key: &RSAPrivateKey) -> Result<(), MyError> {
        let pem_pkcs8 = private_key.to_pem_pkcs8()?;

        info!(
            "INSERT INTO settings (key, value, created_at)
             VALUES ('private_key', $1::TEXT, 'now')
             ON CONFLICT (key)
             DO UPDATE
             SET value = $1::TEXT;"
        );
        self.pool
            .get()
            .await?
            .execute(
                "INSERT INTO settings (key, value, created_at)
                 VALUES ('private_key', $1::TEXT, 'now')
                 ON CONFLICT (key)
                 DO UPDATE
                 SET value = $1::TEXT;",
                &[&pem_pkcs8],
            )
            .await?;
        Ok(())
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
             LISTEN new_nodes;
             LISTEN rm_blocks;
             LISTEN rm_whitelists;
             LISTEN rm_listeners;
             LISTEN rm_actors;
             LISTEN rm_nodes",
        )
        .await?;

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

fn parse_rows<T, E>(rows: Vec<Row>) -> Result<HashSet<T>, MyError>
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
