use crate::{data::State, error::Error};
use actix_web::{web, HttpResponse};

pub(crate) async fn route(state: web::Data<State>) -> Result<HttpResponse, Error> {
    state.db.check_health().await?;
    Ok(HttpResponse::Ok().finish())
}
