use crate::{config::Config, data::State, error::MyError};
use actix_web::{web, HttpResponse};
use log::error;
use std::io::BufWriter;

pub async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<HttpResponse, MyError> {
    let nodes = state.node_cache().nodes().await;

    let mut buf = BufWriter::new(Vec::new());

    crate::templates::index(&mut buf, &nodes, &config)?;
    let buf = buf.into_inner().map_err(|e| {
        error!("Error rendering template, {}", e.error());
        MyError::FlushBuffer
    })?;

    Ok(HttpResponse::Ok().content_type("text/html").body(buf))
}
