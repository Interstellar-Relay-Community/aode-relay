mod actor;
mod healthz;
mod inbox;
mod index;
mod media;
mod nodeinfo;
mod statics;

pub(crate) use self::{
    actor::route as actor,
    healthz::route as healthz,
    inbox::route as inbox,
    index::route as index,
    media::route as media,
    nodeinfo::{route as nodeinfo, well_known as nodeinfo_meta},
    statics::route as statics,
};

use actix_web::HttpResponse;
use serde::ser::Serialize;

static CONTENT_TYPE: &str = "application/activity+json";

fn ok<T>(item: T) -> HttpResponse
where
    T: Serialize,
{
    HttpResponse::Ok().content_type(CONTENT_TYPE).json(&item)
}

fn accepted<T>(item: T) -> HttpResponse
where
    T: Serialize,
{
    HttpResponse::Accepted()
        .content_type(CONTENT_TYPE)
        .json(&item)
}
