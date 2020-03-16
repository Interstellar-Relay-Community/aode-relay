use activitystreams::{actor::apub::Application, context, endpoint::EndpointProperties};
use actix_web::{client::Client, middleware::Logger, web, App, HttpServer, Responder};
use bb8_postgres::tokio_postgres;
use http_signature_normalization_actix::prelude::{VerifyDigest, VerifySignature};
use rsa_pem::KeyExt;
use sha2::{Digest, Sha256};

mod apub;
mod db;
mod db_actor;
mod error;
mod inbox;
mod label;
mod notify;
mod state;
mod verifier;
mod webfinger;

use self::{
    apub::PublicKey,
    db_actor::DbActor,
    error::MyError,
    label::ArbiterLabelFactory,
    state::{State, UrlKind},
    verifier::MyVerify,
    webfinger::RelayResolver,
};

async fn index() -> impl Responder {
    "hewwo, mr obama"
}

async fn actor_route(state: web::Data<State>) -> Result<impl Responder, MyError> {
    let mut application = Application::default();
    let mut endpoint = EndpointProperties::default();

    endpoint.set_shared_inbox(state.generate_url(UrlKind::Inbox))?;

    application
        .object_props
        .set_id(state.generate_url(UrlKind::Actor))?
        .set_summary_xsd_string("AodeRelay bot")?
        .set_name_xsd_string("AodeRelay")?
        .set_url_xsd_any_uri(state.generate_url(UrlKind::Actor))?
        .set_context_xsd_any_uri(context())?;

    application
        .ap_actor_props
        .set_preferred_username("relay")?
        .set_followers(state.generate_url(UrlKind::Followers))?
        .set_following(state.generate_url(UrlKind::Following))?
        .set_inbox(state.generate_url(UrlKind::Inbox))?
        .set_endpoints(endpoint)?;

    let public_key = PublicKey {
        id: state.generate_url(UrlKind::MainKey).parse()?,
        owner: state.generate_url(UrlKind::Actor).parse()?,
        public_key_pem: state.settings.public_key.to_pem_pkcs8()?,
    };

    Ok(inbox::response(public_key.extend(application)))
}

#[actix_rt::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    pretty_env_logger::init();

    let pg_config: tokio_postgres::Config = std::env::var("DATABASE_URL")?.parse()?;
    let hostname: String = std::env::var("HOSTNAME")?;
    let use_whitelist = std::env::var("USE_WHITELIST").is_ok();
    let use_https = std::env::var("USE_HTTPS").is_ok();

    let arbiter_labeler = ArbiterLabelFactory::new();

    let db_actor = DbActor::new(pg_config.clone());
    arbiter_labeler.clone().set_label();

    let state: State = db_actor
        .send(db_actor::DbQuery(move |pool| {
            State::hydrate(use_https, use_whitelist, hostname, pool)
        }))
        .await?
        .await??;

    let _ = notify::NotifyHandler::start_handler(state.clone(), pg_config.clone());

    HttpServer::new(move || {
        let actor = DbActor::new(pg_config.clone());
        arbiter_labeler.clone().set_label();
        let client = Client::default();

        App::new()
            .wrap(Logger::default())
            .data(actor)
            .data(state.clone())
            .data(client.clone())
            .service(web::resource("/").route(web::get().to(index)))
            .service(
                web::resource("/inbox")
                    .wrap(VerifyDigest::new(Sha256::new()))
                    .wrap(VerifySignature::new(
                        MyVerify(state.clone(), client),
                        Default::default(),
                    ))
                    .route(web::post().to(inbox::inbox)),
            )
            .service(web::resource("/actor").route(web::get().to(actor_route)))
            .service(actix_webfinger::resource::<_, RelayResolver>())
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;
    Ok(())
}
