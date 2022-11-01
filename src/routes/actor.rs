use crate::{
    apub::{PublicKey, PublicKeyInner},
    config::{Config, UrlKind},
    data::State,
    error::Error,
    routes::ok,
};
use activitystreams::{
    actor::{ApActor, Application, Endpoints},
    context,
    prelude::*,
    security,
};
use activitystreams_ext::Ext1;
use actix_web::{web, Responder};
use rsa::pkcs8::EncodePublicKey;

#[tracing::instrument(name = "Actor", skip(config, state))]
pub(crate) async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<impl Responder, Error> {
    let mut application = Ext1::new(
        ApActor::new(config.generate_url(UrlKind::Inbox), Application::new()),
        PublicKey {
            public_key: PublicKeyInner {
                id: config.generate_url(UrlKind::MainKey),
                owner: config.generate_url(UrlKind::Actor),
                public_key_pem: state
                    .public_key
                    .to_public_key_pem(rsa::pkcs8::LineEnding::default())?,
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
