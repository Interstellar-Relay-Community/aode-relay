use crate::{
    admin::{AllowedDomains, BlockedDomains, ConnectedActors, Domains, LastSeen},
    collector::Snapshot,
    config::{AdminUrlKind, Config},
    error::{Error, ErrorKind},
};
use awc::Client;
use serde::de::DeserializeOwned;

pub(crate) async fn allow(
    client: &Client,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Allow).await
}

pub(crate) async fn disallow(
    client: &Client,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Disallow).await
}

pub(crate) async fn block(
    client: &Client,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Block).await
}

pub(crate) async fn unblock(
    client: &Client,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Unblock).await
}

pub(crate) async fn allowed(client: &Client, config: &Config) -> Result<AllowedDomains, Error> {
    get_results(client, config, AdminUrlKind::Allowed).await
}

pub(crate) async fn blocked(client: &Client, config: &Config) -> Result<BlockedDomains, Error> {
    get_results(client, config, AdminUrlKind::Blocked).await
}

pub(crate) async fn connected(client: &Client, config: &Config) -> Result<ConnectedActors, Error> {
    get_results(client, config, AdminUrlKind::Connected).await
}

pub(crate) async fn stats(client: &Client, config: &Config) -> Result<Snapshot, Error> {
    get_results(client, config, AdminUrlKind::Stats).await
}

pub(crate) async fn last_seen(client: &Client, config: &Config) -> Result<LastSeen, Error> {
    get_results(client, config, AdminUrlKind::LastSeen).await
}

async fn get_results<T: DeserializeOwned>(
    client: &Client,
    config: &Config,
    url_kind: AdminUrlKind,
) -> Result<T, Error> {
    let x_api_token = config.x_api_token().ok_or(ErrorKind::MissingApiToken)?;

    let iri = config.generate_admin_url(url_kind);

    let mut res = client
        .get(iri.as_str())
        .insert_header(x_api_token)
        .send()
        .await
        .map_err(|e| ErrorKind::SendRequest(iri.to_string(), e.to_string()))?;

    if !res.status().is_success() {
        return Err(ErrorKind::Status(iri.to_string(), res.status()).into());
    }

    let t = res
        .json()
        .await
        .map_err(|e| ErrorKind::ReceiveResponse(iri.to_string(), e.to_string()))?;

    Ok(t)
}

async fn post_domains(
    client: &Client,
    config: &Config,
    domains: Vec<String>,
    url_kind: AdminUrlKind,
) -> Result<(), Error> {
    let x_api_token = config.x_api_token().ok_or(ErrorKind::MissingApiToken)?;

    let iri = config.generate_admin_url(url_kind);

    let res = client
        .post(iri.as_str())
        .insert_header(x_api_token)
        .send_json(&Domains { domains })
        .await
        .map_err(|e| ErrorKind::SendRequest(iri.to_string(), e.to_string()))?;

    if !res.status().is_success() {
        tracing::warn!("Failed to allow domains");
    }

    Ok(())
}
