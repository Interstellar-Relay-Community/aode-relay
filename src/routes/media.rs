use crate::{data::MediaCache, error::Error, requests::Requests};
use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpResponse,
};
use uuid::Uuid;

#[tracing::instrument(name = "Media")]
pub(crate) async fn route(
    media: web::Data<MediaCache>,
    requests: web::Data<Requests>,
    uuid: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let uuid = uuid.into_inner();

    if let Some((content_type, bytes)) = media.get_bytes(uuid).await? {
        return Ok(cached(content_type, bytes));
    }

    if let Some(url) = media.get_url(uuid).await? {
        let (content_type, bytes) = requests.fetch_bytes(url.as_str()).await?;

        media
            .store_bytes(uuid, content_type.clone(), bytes.clone())
            .await?;

        return Ok(cached(content_type, bytes));
    }

    Ok(HttpResponse::NotFound().finish())
}

fn cached(content_type: String, bytes: web::Bytes) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(CacheControl(vec![
            CacheDirective::Public,
            CacheDirective::MaxAge(60 * 60 * 24),
            CacheDirective::Extension("immutable".to_owned(), None),
        ]))
        .content_type(content_type)
        .body(bytes)
}
