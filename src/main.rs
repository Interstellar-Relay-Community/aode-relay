use actix_web::{
    middleware::{Compress, Logger},
    web, App, HttpServer,
};

mod apub;
mod args;
mod config;
mod data;
mod db;
mod error;
mod jobs;
mod middleware;
mod requests;
mod routes;

use self::{
    args::Args,
    config::Config,
    data::{ActorCache, MediaCache, State},
    db::Db,
    jobs::{create_server, create_workers},
    middleware::{DebugPayload, RelayResolver},
    routes::{actor, inbox, index, nodeinfo, nodeinfo_meta, statics},
};

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();

    let config = Config::build()?;

    if config.debug() {
        std::env::set_var(
            "RUST_LOG",
            "debug,h2=info,trust_dns_resolver=info,trust_dns_proto=info,rustls=info,html5ever=info",
        )
    } else {
        std::env::set_var("RUST_LOG", "info")
    }

    if config.pretty_log() {
        pretty_env_logger::init();
    } else {
        env_logger::init();
    }

    let db = Db::build(&config)?;

    let args = Args::new();

    if !args.blocks().is_empty() || !args.allowed().is_empty() {
        if args.undo() {
            db.remove_blocks(args.blocks().to_vec()).await?;
            db.remove_allows(args.allowed().to_vec()).await?;
        } else {
            db.add_blocks(args.blocks().to_vec()).await?;
            db.add_allows(args.allowed().to_vec()).await?;
        }
        return Ok(());
    }

    let media = MediaCache::new(db.clone());
    let state = State::build(config.clone(), db.clone()).await?;
    let actors = ActorCache::new(db.clone());
    let job_server = create_server();

    create_workers(
        db.clone(),
        state.clone(),
        actors.clone(),
        job_server.clone(),
        media.clone(),
        config.clone(),
    );

    let bind_address = config.bind_address();
    HttpServer::new(move || {
        App::new()
            .wrap(Compress::default())
            .wrap(Logger::default())
            .app_data(web::Data::new(db.clone()))
            .app_data(web::Data::new(state.clone()))
            .app_data(web::Data::new(state.requests()))
            .app_data(web::Data::new(actors.clone()))
            .app_data(web::Data::new(config.clone()))
            .app_data(web::Data::new(job_server.clone()))
            .app_data(web::Data::new(media.clone()))
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/media/{path}").route(web::get().to(routes::media)))
            .service(
                web::resource("/inbox")
                    .wrap(config.digest_middleware())
                    .wrap(config.signature_middleware(
                        state.requests(),
                        actors.clone(),
                        state.clone(),
                    ))
                    .wrap(DebugPayload(config.debug()))
                    .route(web::post().to(inbox)),
            )
            .service(web::resource("/actor").route(web::get().to(actor)))
            .service(web::resource("/nodeinfo/2.0.json").route(web::get().to(nodeinfo)))
            .service(
                web::scope("/.well-known")
                    .service(actix_webfinger::scoped::<RelayResolver>())
                    .service(web::resource("/nodeinfo").route(web::get().to(nodeinfo_meta))),
            )
            .service(web::resource("/static/{filename}").route(web::get().to(statics)))
    })
    .bind(bind_address)?
    .run()
    .await?;
    Ok(())
}

include!(concat!(env!("OUT_DIR"), "/templates.rs"));
