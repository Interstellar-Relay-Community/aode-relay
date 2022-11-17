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

pub(crate) async fn block(
    admin: Admin,
    Json(Domains { domains }): Json<Domains>,
) -> Result<HttpResponse, Error> {
    admin.db_ref().add_blocks(domains).await?;

    Ok(HttpResponse::NoContent().finish())
}

pub(crate) async fn allowed(admin: Admin) -> Result<HttpResponse, Error> {
    let allowed_domains = admin.db_ref().allowed_domains().await?;

    Ok(HttpResponse::Ok().json(AllowedDomains { allowed_domains }))
}

pub(crate) async fn blocked(admin: Admin) -> Result<HttpResponse, Error> {
    let blocked_domains = admin.db_ref().blocks().await?;

    Ok(HttpResponse::Ok().json(BlockedDomains { blocked_domains }))
}

pub(crate) async fn connected(admin: Admin) -> Result<HttpResponse, Error> {
    let connected_actors = admin.db_ref().connected_ids().await?;

    Ok(HttpResponse::Ok().json(ConnectedActors { connected_actors }))
}
