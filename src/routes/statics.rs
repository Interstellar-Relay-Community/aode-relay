use crate::templates::statics::StaticFile;
use actix_web::{
    http::header::{ContentType, Expires},
    web, HttpResponse,
};
use std::time::{Duration, SystemTime};

static FAR: Duration = Duration::from_secs(60 * 60 * 24);

pub async fn route(filename: web::Path<String>) -> HttpResponse {
    if let Some(data) = StaticFile::get(&filename.into_inner()) {
        let far_expires = SystemTime::now() + FAR;
        HttpResponse::Ok()
            .set(Expires(far_expires.into()))
            .set(ContentType(data.mime.clone()))
            .body(data.content)
    } else {
        HttpResponse::NotFound()
            .reason("No such static file.")
            .finish()
    }
}
