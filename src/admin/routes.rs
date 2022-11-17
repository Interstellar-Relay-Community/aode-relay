use crate::{
    admin::{AllowedDomains, BlockedDomains, ConnectedActors, Domains},
    error::Error,
    extractors::Admin,
};
use actix_web::{web::Json, HttpResponse};

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
