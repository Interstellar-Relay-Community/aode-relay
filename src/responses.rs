use actix_web::HttpResponse;
use serde::ser::Serialize;

static CONTENT_TYPE: &str = "application/activity+json";

pub fn ok<T>(item: T) -> HttpResponse
where
    T: Serialize,
{
    HttpResponse::Ok().content_type(CONTENT_TYPE).json(item)
}

pub fn accepted<T>(item: T) -> HttpResponse
where
    T: Serialize,
{
    HttpResponse::Accepted()
        .content_type(CONTENT_TYPE)
        .json(item)
}
