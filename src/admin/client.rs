use crate::{
    admin::{AllowedDomains, BlockedDomains, ConnectedActors, Domains, LastSeen},
    collector::Snapshot,
    config::{AdminUrlKind, Config},
    error::{Error, ErrorKind},
    extractors::XApiToken,
};
use actix_web::http::header::Header;
use reqwest_middleware::ClientWithMiddleware;
use serde::de::DeserializeOwned;

pub(crate) async fn allow(
    client: &ClientWithMiddleware,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Allow).await
}

pub(crate) async fn disallow(
    client: &ClientWithMiddleware,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Disallow).await
}

pub(crate) async fn block(
    client: &ClientWithMiddleware,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Block).await
}

pub(crate) async fn unblock(
    client: &ClientWithMiddleware,
    config: &Config,
    domains: Vec<String>,
) -> Result<(), Error> {
    post_domains(client, config, domains, AdminUrlKind::Unblock).await
}

pub(crate) async fn allowed(
    client: &ClientWithMiddleware,
    config: &Config,
) -> Result<AllowedDomains, Error> {
    get_results(client, config, AdminUrlKind::Allowed).await
}

pub(crate) async fn blocked(
    client: &ClientWithMiddleware,
    config: &Config,
) -> Result<BlockedDomains, Error> {
    get_results(client, config, AdminUrlKind::Blocked).await
}

pub(crate) async fn connected(
    client: &ClientWithMiddleware,
    config: &Config,
) -> Result<ConnectedActors, Error> {
    get_results(client, config, AdminUrlKind::Connected).await
}

pub(crate) async fn stats(
    client: &ClientWithMiddleware,
    config: &Config,
) -> Result<Snapshot, Error> {
    get_results(client, config, AdminUrlKind::Stats).await
}

pub(crate) async fn last_seen(
    client: &ClientWithMiddleware,
    config: &Config,
) -> Result<LastSeen, Error> {
    get_results(client, config, AdminUrlKind::LastSeen).await
}

async fn get_results<T: DeserializeOwned>(
    client: &ClientWithMiddleware,
    config: &Config,
    url_kind: AdminUrlKind,
) -> Result<T, Error> {
    let x_api_token = config.x_api_token().ok_or(ErrorKind::MissingApiToken)?;

    let iri = config.generate_admin_url(url_kind);

    let res = client
        .get(iri.as_str())
        .header(XApiToken::name(), x_api_token.to_string())
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
    client: &ClientWithMiddleware,
    config: &Config,
    domains: Vec<String>,
    url_kind: AdminUrlKind,
) -> Result<(), Error> {
    let x_api_token = config.x_api_token().ok_or(ErrorKind::MissingApiToken)?;

    let iri = config.generate_admin_url(url_kind);

    let res = client
        .post(iri.as_str())
        .header(XApiToken::name(), x_api_token.to_string())
        .json(&Domains { domains })
        .send()
        .await
        .map_err(|e| ErrorKind::SendRequest(iri.to_string(), e.to_string()))?;

    if !res.status().is_success() {
        tracing::warn!("Failed to allow domains");
    }

    Ok(())
}
