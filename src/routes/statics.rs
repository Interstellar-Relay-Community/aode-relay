use crate::templates::statics::StaticFile;
use actix_web::{
    http::header::{CacheControl, CacheDirective, ContentType},
    web, HttpResponse,
};

pub async fn route(filename: web::Path<String>) -> HttpResponse {
    if let Some(data) = StaticFile::get(&filename.into_inner()) {
        HttpResponse::Ok()
            .set(CacheControl(vec![
                CacheDirective::Public,
                CacheDirective::MaxAge(60 * 60 * 24),
                CacheDirective::Extension("immutable".to_owned(), None),
            ]))
            .set(ContentType(data.mime.clone()))
            .body(data.content)
    } else {
        HttpResponse::NotFound()
            .reason("No such static file.")
            .finish()
    }
}
