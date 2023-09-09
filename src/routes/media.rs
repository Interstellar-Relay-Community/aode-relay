use crate::{
    data::MediaCache,
    error::Error,
    requests::{BreakerStrategy, Requests},
};
use actix_web::{body::BodyStream, web, HttpResponse};
use uuid::Uuid;

#[tracing::instrument(name = "Media", skip(media, requests))]
pub(crate) async fn route(
    media: web::Data<MediaCache>,
    requests: web::Data<Requests>,
    uuid: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let uuid = uuid.into_inner();

    if let Some(url) = media.get_url(uuid).await? {
        let res = requests
            .fetch_response(&url, BreakerStrategy::Allow404AndBelow)
            .await?;

        let mut response = HttpResponse::build(res.status());

        for (name, value) in res.headers().iter().filter(|(h, _)| *h != "connection") {
            response.insert_header((name.clone(), value.clone()));
        }

        return Ok(response.body(BodyStream::new(res.bytes_stream())));
    }

    Ok(HttpResponse::NotFound().finish())
}
