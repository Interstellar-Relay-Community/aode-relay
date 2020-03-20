use actix_web::{middleware::Logger, web, App, HttpServer, Responder};
use bb8_postgres::tokio_postgres;

mod actor;
mod apub;
mod args;
mod config;
mod db;
mod error;
mod inbox;
mod nodeinfo;
mod notify;
mod rehydrate;
mod requests;
mod responses;
mod state;
mod verifier;
mod webfinger;

use self::{args::Args, config::Config, db::Db, state::State, webfinger::RelayResolver};

async fn index(state: web::Data<State>, config: web::Data<Config>) -> impl Responder {
    let mut s = String::new();
    s.push_str(&format!("Welcome to the relay on {}\n", config.hostname()));

    let listeners = state.listeners().await;
    if listeners.is_empty() {
        s.push_str("There are no currently connected servers\n");
    } else {
        s.push_str("Here are the currently connected servers:\n");
        s.push_str("\n");
    }

    for listener in listeners {
        if let Some(domain) = listener.as_url().domain() {
            s.push_str(&format!("{}\n", domain));
        }
    }
    s.push_str("\n");
    s.push_str(&format!(
        "The source code for this project can be found at {}\n",
        config.source_code()
    ));

    s
}

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();

    let config = Config::build()?;

    if config.debug() {
        std::env::set_var("RUST_LOG", "debug")
    } else {
        std::env::set_var("RUST_LOG", "info")
    }

    if config.pretty_log() {
        pretty_env_logger::init();
    } else {
        env_logger::init();
    }

    let pg_config: tokio_postgres::Config = config.database_url().parse()?;
    let db = Db::build(pg_config.clone()).await?;

    let args = Args::new();

    if !args.blocks().is_empty() || !args.whitelists().is_empty() {
        if args.undo() {
            db.remove_blocks(args.blocks()).await?;
            db.remove_whitelists(args.whitelists()).await?;
        } else {
            db.add_blocks(args.blocks()).await?;
            db.add_whitelists(args.whitelists()).await?;
        }
        return Ok(());
    }

    let state = State::hydrate(config.clone(), &db).await?;

    rehydrate::spawn(db.clone(), state.clone());

    let _ = notify::NotifyHandler::start_handler(state.clone(), pg_config.clone());

    let bind_address = config.bind_address();
    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .data(db.clone())
            .data(state.clone())
            .data(state.requests())
            .data(config.clone())
            .service(web::resource("/").route(web::get().to(index)))
            .service(
                web::resource("/inbox")
                    .wrap(config.digest_middleware())
                    .wrap(config.signature_middleware(state.requests()))
                    .route(web::post().to(inbox::inbox)),
            )
            .service(web::resource("/actor").route(web::get().to(actor::route)))
            .service(web::resource("/nodeinfo/2.0").route(web::get().to(nodeinfo::route)))
            .service(
                web::scope("/.well-known")
                    .service(actix_webfinger::scoped::<_, RelayResolver>())
                    .service(web::resource("/nodeinfo").route(web::get().to(nodeinfo::well_known))),
            )
    })
    .bind(bind_address)?
    .run()
    .await?;
    Ok(())
}
