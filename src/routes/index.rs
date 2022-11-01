use crate::{
    config::Config,
    data::State,
    error::{Error, ErrorKind},
};
use actix_web::{web, HttpResponse};
use rand::{seq::SliceRandom, thread_rng};
use std::io::BufWriter;
use tracing::error;

#[tracing::instrument(name = "Index", skip(config, state))]
pub(crate) async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<HttpResponse, Error> {
    let mut nodes = state.node_cache().nodes().await?;
    nodes.shuffle(&mut thread_rng());
    let mut buf = BufWriter::new(Vec::new());

    crate::templates::index(&mut buf, &nodes, &config)?;
    let buf = buf.into_inner().map_err(|e| {
        error!("Error rendering template, {}", e.error());
        ErrorKind::FlushBuffer
    })?;

    Ok(HttpResponse::Ok().content_type("text/html").body(buf))
}
