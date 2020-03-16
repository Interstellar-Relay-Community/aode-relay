use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use bb8_postgres::tokio_postgres::{row::Row, Client};
use log::info;
use rsa::RSAPrivateKey;
use rsa_pem::KeyExt;
use std::collections::HashSet;

#[derive(Clone, Debug, thiserror::Error)]
#[error("No host present in URI")]
pub struct HostError;

pub async fn listen(client: &Client) -> Result<(), Error> {
    info!("LISTEN new_blocks;");
    info!("LISTEN new_whitelists;");
    info!("LISTEN new_listeners;");
    info!("LISTEN rm_blocks;");
    info!("LISTEN rm_whitelists;");
    info!("LISTEN rm_listeners;");
    client
        .batch_execute(
            "LISTEN new_blocks;
             LISTEN new_whitelists;
             LISTEN new_listeners;
             LISTEN rm_blocks;
             LISTEN rm_whitelists;
             LISTEN rm_listeners;",
        )
        .await?;

    Ok(())
}

pub async fn hydrate_private_key(client: &Client) -> Result<Option<RSAPrivateKey>, Error> {
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

pub async fn update_private_key(client: &Client, key: &RSAPrivateKey) -> Result<(), Error> {
    let pem_pkcs8 = key.to_pem_pkcs8()?;

    info!("INSERT INTO settings (key, value, created_at) VALUES ('private_key', $1::TEXT, 'now');");
    client.execute("INSERT INTO settings (key, value, created_at) VALUES ('private_key', $1::TEXT, 'now');", &[&pem_pkcs8]).await?;
    Ok(())
}

pub async fn add_block(client: &Client, block: &XsdAnyUri) -> Result<(), Error> {
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

    Ok(())
}

pub async fn add_whitelist(client: &Client, whitelist: &XsdAnyUri) -> Result<(), Error> {
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

    Ok(())
}

pub async fn remove_listener(client: &Client, listener: &XsdAnyUri) -> Result<(), Error> {
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

pub async fn add_listener(client: &Client, listener: &XsdAnyUri) -> Result<(), Error> {
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

fn parse_rows<T>(rows: Vec<Row>) -> Result<HashSet<T>, Error>
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
