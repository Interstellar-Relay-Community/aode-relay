use crate::{
    apub::{PublicKey, PublicKeyInner},
    config::{Config, UrlKind},
    data::State,
    error::MyError,
    routes::ok,
};
use activitystreams_ext::Ext1;
use activitystreams_new::{
    actor::{ApActor, Application, Endpoints},
    context,
    prelude::*,
    security,
};
use actix_web::{web, Responder};
use rsa_pem::KeyExt;

pub async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<impl Responder, MyError> {
    let mut application = Ext1::new(
        ApActor::new(config.generate_url(UrlKind::Inbox), Application::new()),
        PublicKey {
            public_key: PublicKeyInner {
                id: config.generate_url(UrlKind::MainKey),
                owner: config.generate_url(UrlKind::Actor),
                public_key_pem: state.public_key.to_pem_pkcs8()?,
            },
        },
    );

    application
        .set_id(config.generate_url(UrlKind::Actor))
        .set_summary("AodeRelay bot")
        .set_name("AodeRelay")
        .set_url(config.generate_url(UrlKind::Actor))
        .set_many_contexts(vec![context(), security()])
        .set_preferred_username("relay")
        .set_outbox(config.generate_url(UrlKind::Outbox))
        .set_followers(config.generate_url(UrlKind::Followers))
        .set_following(config.generate_url(UrlKind::Following))
        .set_endpoints(Endpoints {
            shared_inbox: Some(config.generate_url(UrlKind::Inbox)),
            ..Default::default()
        });

    Ok(ok(application))
}
