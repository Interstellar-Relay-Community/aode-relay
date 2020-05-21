use crate::{data::Media, error::MyError, requests::Requests};
use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpResponse,
};
use bytes::Bytes;
use uuid::Uuid;

pub async fn route(
    media: web::Data<Media>,
    requests: web::Data<Requests>,
    uuid: web::Path<Uuid>,
) -> Result<HttpResponse, MyError> {
    let uuid = uuid.into_inner();

    if let Some((content_type, bytes)) = media.get_bytes(uuid).await {
        return Ok(cached(content_type, bytes));
    }

    if let Some(url) = media.get_url(uuid).await? {
        let (content_type, bytes) = requests.fetch_bytes(url.as_str()).await?;

        media
            .store_bytes(uuid, content_type.clone(), bytes.clone())
            .await;

        return Ok(cached(content_type, bytes));
    }

    Ok(HttpResponse::NotFound().finish())
}

fn cached(content_type: String, bytes: Bytes) -> HttpResponse {
    HttpResponse::Ok()
        .set(CacheControl(vec![
            CacheDirective::Public,
            CacheDirective::MaxAge(60 * 60 * 24),
            CacheDirective::Extension("immutable".to_owned(), None),
        ]))
        .content_type(content_type)
        .body(bytes)
}
