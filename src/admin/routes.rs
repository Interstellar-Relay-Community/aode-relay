use crate::{
    admin::{AllowedDomains, BlockedDomains, ConnectedActors, Domains, LastSeen},
    collector::{MemoryCollector, Snapshot},
    error::Error,
    extractors::Admin,
};
use actix_web::{
    web::{self, Data, Json},
    HttpResponse,
};
use std::collections::{BTreeMap, BTreeSet};
use time::OffsetDateTime;

pub(crate) async fn allow(
    admin: Admin,
    Json(Domains { domains }): Json<Domains>,
) -> Result<HttpResponse, Error> {
    admin.db_ref().add_allows(domains).await?;

    Ok(HttpResponse::NoContent().finish())
}

pub(crate) async fn disallow(
    admin: Admin,
    Json(Domains { domains }): Json<Domains>,
) -> Result<HttpResponse, Error> {
    admin.db_ref().remove_allows(domains).await?;

    Ok(HttpResponse::NoContent().finish())
}

pub(crate) async fn block(
    admin: Admin,
    Json(Domains { domains }): Json<Domains>,
) -> Result<HttpResponse, Error> {
    admin.db_ref().add_blocks(domains).await?;

    Ok(HttpResponse::NoContent().finish())
}

pub(crate) async fn unblock(
    admin: Admin,
    Json(Domains { domains }): Json<Domains>,
) -> Result<HttpResponse, Error> {
    admin.db_ref().remove_blocks(domains).await?;

    Ok(HttpResponse::NoContent().finish())
}

pub(crate) async fn allowed(admin: Admin) -> Result<Json<AllowedDomains>, Error> {
    let allowed_domains = admin.db_ref().allows().await?;

    Ok(Json(AllowedDomains { allowed_domains }))
}

pub(crate) async fn blocked(admin: Admin) -> Result<Json<BlockedDomains>, Error> {
    let blocked_domains = admin.db_ref().blocks().await?;

    Ok(Json(BlockedDomains { blocked_domains }))
}

pub(crate) async fn connected(admin: Admin) -> Result<Json<ConnectedActors>, Error> {
    let connected_actors = admin.db_ref().connected_ids().await?;

    Ok(Json(ConnectedActors { connected_actors }))
}

pub(crate) async fn stats(
    _admin: Admin,
    collector: Data<MemoryCollector>,
) -> Result<Json<Snapshot>, Error> {
    Ok(Json(collector.snapshot()))
}

pub(crate) async fn last_seen(admin: Admin) -> Result<Json<LastSeen>, Error> {
    let nodes = admin.db_ref().last_seen().await?;

    let mut last_seen: BTreeMap<OffsetDateTime, BTreeSet<String>> = BTreeMap::new();
    let mut never = Vec::new();

    for (domain, datetime) in nodes {
        if let Some(datetime) = datetime {
            last_seen.entry(datetime).or_default().insert(domain);
        } else {
            never.push(domain);
        }
    }

    Ok(Json(LastSeen { last_seen, never }))
}

pub(crate) async fn get_authority_cfg(
    _admin: Admin,
    state: Data<crate::data::State>,
    domain: web::Path<String>,
) -> Result<Json<crate::data::NodeConfig>, Error> {
    if let Some(cfg) = state.get_authority_cfg(&domain).await {
        Ok(Json(cfg))
    } else {
        Err(crate::error::ErrorKind::NotFound.into())
    }
}

pub(crate) async fn get_all_authority_cfg(
    _admin: Admin,
    state: Data<crate::data::State>,
) -> Result<Json<std::collections::HashMap<String, crate::data::NodeConfig>>, Error> {
    let cfg = state.get_all_authority_cfg().await;

    Ok(Json(cfg))
}

pub(crate) async fn set_authority_cfg(
    _admin: Admin,
    state: Data<crate::data::State>,
    domain: web::Path<String>,
    Json(cfg): Json<crate::data::NodeConfig>,
) -> Result<HttpResponse, Error> {
    state.set_authority_cfg(&domain, cfg).await;

    Ok(HttpResponse::NoContent().finish())
}

pub(crate) async fn clear_authority_cfg(
    _admin: Admin,
    state: Data<crate::data::State>,
    domain: web::Path<String>,
) -> Result<HttpResponse, Error> {
    state.clear_authority_cfg(&domain).await;

    Ok(HttpResponse::NoContent().finish())
}
