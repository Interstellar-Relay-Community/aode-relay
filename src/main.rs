use actix::Arbiter;
use actix_web::{
    http::header::{ContentType, Expires},
    middleware::Logger,
    web, App, HttpResponse, HttpServer,
};
use log::error;
use std::{
    io::BufWriter,
    time::{Duration, SystemTime},
};

mod actor;
mod apub;
mod args;
mod config;
mod db;
mod error;
mod inbox;
mod jobs;
mod nodeinfo;
mod notify;
mod rehydrate;
mod requests;
mod responses;
mod state;
mod verifier;
mod webfinger;

use self::{
    args::Args,
    config::Config,
    db::Db,
    error::MyError,
    jobs::{create_server, create_workers},
    state::State,
    templates::statics::StaticFile,
    webfinger::RelayResolver,
};

async fn index(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<HttpResponse, MyError> {
    let listeners = state.listeners().await;

    let mut buf = BufWriter::new(Vec::new());

    templates::index(&mut buf, &listeners, &config)?;
    let buf = buf.into_inner().map_err(|e| {
        error!("Error rendering template, {}", e.error());
        MyError::FlushBuffer
    })?;

    Ok(HttpResponse::Ok().content_type("text/html").body(buf))
}

static FAR: Duration = Duration::from_secs(60 * 60 * 24);

async fn static_file(filename: web::Path<String>) -> HttpResponse {
    if let Some(data) = StaticFile::get(&filename.into_inner()) {
        let far_expires = SystemTime::now() + FAR;
        HttpResponse::Ok()
            .set(Expires(far_expires.into()))
            .set(ContentType(data.mime.clone()))
            .body(data.content)
    } else {
        HttpResponse::NotFound()
            .reason("No such static file.")
            .finish()
    }
}

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

    rehydrate::spawn(db.clone(), state.clone());
    notify::spawn(state.clone(), &config)?;

    let job_server = create_server(db.clone());

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
                    .route(web::post().to(inbox::inbox)),
            )
            .service(web::resource("/actor").route(web::get().to(actor::route)))
            .service(web::resource("/nodeinfo/2.0.json").route(web::get().to(nodeinfo::route)))
            .service(
                web::scope("/.well-known")
                    .service(actix_webfinger::scoped::<_, RelayResolver>())
                    .service(web::resource("/nodeinfo").route(web::get().to(nodeinfo::well_known))),
            )
            .service(web::resource("/static/{filename}").route(web::get().to(static_file)))
    })
    .bind(bind_address)?
    .run()
    .await?;
    Ok(())
}

include!(concat!(env!("OUT_DIR"), "/templates.rs"));
