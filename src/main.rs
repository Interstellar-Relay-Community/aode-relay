use actix::Arbiter;
use actix_web::{middleware::Logger, web, App, HttpServer};

mod apub;
mod args;
mod config;
mod db;
mod error;
mod jobs;
mod middleware;
mod node;
mod notify;
mod requests;
mod routes;
mod state;

use self::{
    args::Args,
    config::Config,
    db::Db,
    jobs::{create_server, create_workers},
    middleware::RelayResolver,
    routes::{actor, inbox, index, nodeinfo, nodeinfo_meta, statics},
    state::State,
};

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();

    let config = Config::build()?;

    if config.debug() {
        std::env::set_var("RUST_LOG", "debug,tokio_postgres=info")
    } else {
        std::env::set_var("RUST_LOG", "info")
    }

    if config.pretty_log() {
        pretty_env_logger::init();
    } else {
        env_logger::init();
    }

    let db = Db::build(&config).await?;

    let args = Args::new();

    if args.jobs_only() && args.no_jobs() {
        return Err(anyhow::Error::msg(
            "Either the server or the jobs must be run",
        ));
    }

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
    let job_server = create_server(db.clone());

    notify::spawn(state.clone(), job_server.clone(), &config)?;

    if args.jobs_only() {
        for _ in 0..num_cpus::get() {
            let state = state.clone();
            let job_server = job_server.clone();

            Arbiter::new().exec_fn(move || {
                create_workers(state, job_server);
            });
        }
        actix_rt::signal::ctrl_c().await?;
        return Ok(());
    }

    let no_jobs = args.no_jobs();

    let bind_address = config.bind_address();
    HttpServer::new(move || {
        if !no_jobs {
            create_workers(state.clone(), job_server.clone());
        }

        App::new()
            .wrap(Logger::default())
            .data(db.clone())
            .data(state.clone())
            .data(state.requests())
            .data(config.clone())
            .data(job_server.clone())
            .service(web::resource("/").route(web::get().to(index)))
            .service(
                web::resource("/inbox")
                    .wrap(config.digest_middleware())
                    .wrap(config.signature_middleware(state.requests()))
                    .route(web::post().to(inbox)),
            )
            .service(web::resource("/actor").route(web::get().to(actor)))
            .service(web::resource("/nodeinfo/2.0.json").route(web::get().to(nodeinfo)))
            .service(
                web::scope("/.well-known")
                    .service(actix_webfinger::scoped::<_, RelayResolver>())
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
