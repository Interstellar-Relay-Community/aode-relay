// need this for ructe
#![allow(clippy::needless_borrow)]

use activitystreams::iri_string::types::IriString;
use actix_rt::task::JoinHandle;
use actix_web::{middleware::Compress, web, App, HttpServer};
use collector::MemoryCollector;
#[cfg(feature = "console")]
use console_subscriber::ConsoleLayer;
use http_signature_normalization_actix::middleware::VerifySignature;
use metrics_exporter_prometheus::PrometheusBuilder;
use metrics_util::layers::FanoutBuilder;
use opentelemetry::{sdk::Resource, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use rustls::ServerConfig;
use tracing_actix_web::TracingLogger;
use tracing_error::ErrorLayer;
use tracing_log::LogTracer;
use tracing_subscriber::{filter::Targets, fmt::format::FmtSpan, layer::SubscriberExt, Layer};

mod admin;
mod apub;
mod args;
mod collector;
mod config;
mod data;
mod db;
mod error;
mod extractors;
mod jobs;
mod middleware;
mod requests;
mod routes;
mod telegram;

use self::{
    args::Args,
    config::Config,
    data::{ActorCache, MediaCache, State},
    db::Db,
    jobs::create_workers,
    middleware::{DebugPayload, MyVerify, RelayResolver, Timings},
    routes::{actor, healthz, inbox, index, nodeinfo, nodeinfo_meta, statics},
};

fn init_subscriber(
    software_name: &'static str,
    opentelemetry_url: Option<&IriString>,
) -> Result<(), anyhow::Error> {
    LogTracer::init()?;

    let targets: Targets = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "warn,actix_web=debug,actix_server=debug,tracing_actix_web=info".into())
        .parse()?;

    let format_layer = tracing_subscriber::fmt::layer()
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_filter(targets.clone());

    #[cfg(feature = "console")]
    let console_layer = ConsoleLayer::builder()
        .with_default_env()
        .server_addr(([0, 0, 0, 0], 6669))
        .event_buffer_capacity(1024 * 1024)
        .spawn();

    let subscriber = tracing_subscriber::Registry::default()
        .with(format_layer)
        .with(ErrorLayer::default());

    #[cfg(feature = "console")]
    let subscriber = subscriber.with(console_layer);

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

        let otel_layer = tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(targets);

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

    let args = Args::new();

    if args.any() {
        return client_main(config, args).await?;
    }

    let collector = MemoryCollector::new();

    if let Some(bind_addr) = config.prometheus_bind_address() {
        let (recorder, exporter) = PrometheusBuilder::new()
            .with_http_listener(bind_addr)
            .build()?;

        actix_rt::spawn(exporter);
        let recorder = FanoutBuilder::default()
            .add_recorder(recorder)
            .add_recorder(collector.clone())
            .build();
        metrics::set_boxed_recorder(Box::new(recorder))?;
    } else {
        collector.install()?;
    }

    tracing::warn!("Opening DB");
    let db = Db::build(&config)?;

    tracing::warn!("Building caches");
    let actors = ActorCache::new(db.clone());
    let media = MediaCache::new(db.clone());

    server_main(db, actors, media, collector, config).await??;

    tracing::warn!("Application exit");

    Ok(())
}

fn client_main(config: Config, args: Args) -> JoinHandle<Result<(), anyhow::Error>> {
    actix_rt::spawn(do_client_main(config, args))
}

async fn do_client_main(config: Config, args: Args) -> Result<(), anyhow::Error> {
    let client = requests::build_client(&config.user_agent(), config.client_pool_size());

    if !args.blocks().is_empty() || !args.allowed().is_empty() {
        if args.undo() {
            admin::client::unblock(&client, &config, args.blocks().to_vec()).await?;
            admin::client::disallow(&client, &config, args.allowed().to_vec()).await?;
        } else {
            admin::client::block(&client, &config, args.blocks().to_vec()).await?;
            admin::client::allow(&client, &config, args.allowed().to_vec()).await?;
        }
        println!("Updated lists");
    }

    if args.contacted() {
        let last_seen = admin::client::last_seen(&client, &config).await?;

        let mut report = String::from("Contacted:");

        if !last_seen.never.is_empty() {
            report += "\nNever seen:\n";
        }

        for domain in last_seen.never {
            report += "\t";
            report += &domain;
            report += "\n";
        }

        if !last_seen.last_seen.is_empty() {
            report += "\nSeen:\n";
        }

        for (datetime, domains) in last_seen.last_seen {
            for domain in domains {
                report += "\t";
                report += &datetime.to_string();
                report += " - ";
                report += &domain;
                report += "\n";
            }
        }

        report += "\n";
        println!("{report}");
    }

    if args.list() {
        let (blocked, allowed, connected) = tokio::try_join!(
            admin::client::blocked(&client, &config),
            admin::client::allowed(&client, &config),
            admin::client::connected(&client, &config)
        )?;

        let mut report = String::from("Report:\n");
        if !allowed.allowed_domains.is_empty() {
            report += "\nAllowed\n\t";
            report += &allowed.allowed_domains.join("\n\t");
        }
        if !blocked.blocked_domains.is_empty() {
            report += "\n\nBlocked\n\t";
            report += &blocked.blocked_domains.join("\n\t");
        }
        if !connected.connected_actors.is_empty() {
            report += "\n\nConnected\n\t";
            report += &connected.connected_actors.join("\n\t");
        }
        report += "\n";
        println!("{report}");
    }

    if args.stats() {
        let stats = admin::client::stats(&client, &config).await?;
        stats.present();
    }

    Ok(())
}

fn server_main(
    db: Db,
    actors: ActorCache,
    media: MediaCache,
    collector: MemoryCollector,
    config: Config,
) -> JoinHandle<Result<(), anyhow::Error>> {
    actix_rt::spawn(do_server_main(db, actors, media, collector, config))
}

async fn do_server_main(
    db: Db,
    actors: ActorCache,
    media: MediaCache,
    collector: MemoryCollector,
    config: Config,
) -> Result<(), anyhow::Error> {
    tracing::warn!("Creating state");
    let state = State::build(db.clone()).await?;

    if let Some((token, admin_handle)) = config.telegram_info() {
        tracing::warn!("Creating telegram handler");
        telegram::start(admin_handle.to_owned(), db.clone(), token);
    }

    let keys = config.open_keys()?;

    let bind_address = config.bind_address();
    let server = HttpServer::new(move || {
        let requests = state.requests(&config);

        let job_server =
            create_workers(state.clone(), actors.clone(), media.clone(), config.clone());

        let app = App::new()
            .app_data(web::Data::new(db.clone()))
            .app_data(web::Data::new(state.clone()))
            .app_data(web::Data::new(requests.clone()))
            .app_data(web::Data::new(actors.clone()))
            .app_data(web::Data::new(config.clone()))
            .app_data(web::Data::new(job_server))
            .app_data(web::Data::new(media.clone()))
            .app_data(web::Data::new(collector.clone()));

        let app = if let Some(data) = config.admin_config() {
            app.app_data(data)
        } else {
            app
        };

        app.wrap(Compress::default())
            .wrap(TracingLogger::default())
            .wrap(Timings)
            .route("/healthz", web::get().to(healthz))
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/media/{path}").route(web::get().to(routes::media)))
            .service(
                web::resource("/inbox")
                    .wrap(config.digest_middleware())
                    .wrap(VerifySignature::new(
                        MyVerify(requests, actors.clone(), state.clone()),
                        Default::default(),
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
            .service(
                web::scope("/api/v1").service(
                    web::scope("/admin")
                        .route("/allow", web::post().to(admin::routes::allow))
                        .route("/disallow", web::post().to(admin::routes::disallow))
                        .route("/block", web::post().to(admin::routes::block))
                        .route("/unblock", web::post().to(admin::routes::unblock))
                        .route("/allowed", web::get().to(admin::routes::allowed))
                        .route("/blocked", web::get().to(admin::routes::blocked))
                        .route("/connected", web::get().to(admin::routes::connected))
                        .route("/stats", web::get().to(admin::routes::stats))
                        .route("/last_seen", web::get().to(admin::routes::last_seen)),
                ),
            )
    });

    if let Some((certs, key)) = keys {
        tracing::warn!("Binding to {}:{} with TLS", bind_address.0, bind_address.1);
        let server_config = ServerConfig::builder()
            .with_safe_default_cipher_suites()
            .with_safe_default_kx_groups()
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        server
            .bind_rustls(bind_address, server_config)?
            .run()
            .await?;
    } else {
        tracing::warn!("Binding to {}:{}", bind_address.0, bind_address.1);
        server.bind(bind_address)?.run().await?;
    }

    tracing::warn!("Server closed");

    Ok(())
}

include!(concat!(env!("OUT_DIR"), "/templates.rs"));
