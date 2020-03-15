#![feature(drain_filter)]
use actix_web::{client::Client, web, App, HttpServer, Responder};
use bb8_postgres::tokio_postgres;

mod apub;
mod db_actor;
mod inbox;
mod label;
mod state;

use self::{db_actor::DbActor, label::ArbiterLabelFactory, state::State};

async fn index() -> impl Responder {
    "hewwo, mr obama"
}

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    std::env::set_var("RUST_LOG", "info");
    pretty_env_logger::init();

    let pg_config: tokio_postgres::Config = std::env::var("DATABASE_URL")?.parse()?;
    let arbiter_labeler = ArbiterLabelFactory::new();

    let db_actor = DbActor::new(pg_config.clone());
    arbiter_labeler.clone().set_label();

    let state: State = db_actor
        .send(db_actor::DbQuery(State::hydrate))
        .await?
        .await??;

    HttpServer::new(move || {
        let actor = DbActor::new(pg_config.clone());
        arbiter_labeler.clone().set_label();
        let client = Client::default();

        App::new()
            .data(actor)
            .data(state.clone())
            .data(client)
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/inbox").route(web::post().to(inbox::inbox)))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;
    Ok(())
}
