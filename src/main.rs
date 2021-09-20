use activitystreams::url::Url;
use actix_web::{web, App, HttpServer};
use opentelemetry::{sdk::Resource, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use tracing_actix_web::TracingLogger;
use tracing_error::ErrorLayer;
use tracing_log::LogTracer;
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, EnvFilter};

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

fn init_subscriber(
    software_name: &'static str,
    opentelemetry_url: Option<&Url>,
) -> Result<(), anyhow::Error> {
    LogTracer::init()?;

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let format_layer = tracing_subscriber::fmt::layer()
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .pretty();

    let subscriber = tracing_subscriber::Registry::default()
        .with(env_filter)
        .with(ErrorLayer::default())
        .with(format_layer);

    if let Some(url) = opentelemetry_url {
        let tracer =
            opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_trace_config(opentelemetry::sdk::trace::config().with_resource(
                    Resource::new(vec![KeyValue::new("service.name", software_name)]),
                ))
                .with_exporter(
                    opentelemetry_otlp::new_exporter()
                        .tonic()
                        .with_endpoint(url.as_str()),
                )
                .install_batch(opentelemetry::runtime::Tokio)?;

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        let subscriber = subscriber.with(otel_layer);
        tracing::subscriber::set_global_default(subscriber)?;
    } else {
        tracing::subscriber::set_global_default(subscriber)?;
    }

    Ok(())
}

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();

    let config = Config::build()?;

    init_subscriber(Config::software_name(), config.opentelemetry_url())?;

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
            .wrap(TracingLogger::default())
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
