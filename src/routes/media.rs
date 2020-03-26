use crate::{data::Media, error::MyError, requests::Requests};
use actix_web::{web, HttpResponse};
use uuid::Uuid;

pub async fn route(
    media: web::Data<Media>,
    requests: web::Data<Requests>,
    uuid: web::Path<Uuid>,
) -> Result<HttpResponse, MyError> {
    let uuid = uuid.into_inner();

    if let Some((content_type, bytes)) = media.get_bytes(uuid).await {
        return Ok(HttpResponse::Ok().content_type(content_type).body(bytes));
    }

    if let Some(url) = media.get_url(uuid).await {
        let (content_type, bytes) = requests.fetch_bytes(url.as_str()).await?;

        media
            .store_bytes(uuid, content_type.clone(), bytes.clone())
            .await;

        return Ok(HttpResponse::Ok()
            .content_type(content_type)
            .header("Cache-Control", "public, max-age=1200, immutable")
            .body(bytes));
    }

    Ok(HttpResponse::NotFound().finish())
}
