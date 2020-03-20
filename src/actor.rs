use crate::{
    apub::PublicKey,
    config::{Config, UrlKind},
    error::MyError,
    responses::ok,
    state::State,
};
use activitystreams::{
    actor::Application, context, endpoint::EndpointProperties, ext::Extensible,
    object::properties::ObjectProperties, security,
};
use actix_web::{web, Responder};
use rsa_pem::KeyExt;

pub async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<impl Responder, MyError> {
    let mut application = Application::full();
    let mut endpoint = EndpointProperties::default();

    endpoint.set_shared_inbox(config.generate_url(UrlKind::Inbox))?;

    let props: &mut ObjectProperties = application.as_mut();
    props
        .set_id(config.generate_url(UrlKind::Actor))?
        .set_summary_xsd_string("AodeRelay bot")?
        .set_name_xsd_string("AodeRelay")?
        .set_url_xsd_any_uri(config.generate_url(UrlKind::Actor))?
        .set_many_context_xsd_any_uris(vec![context(), security()])?;

    application
        .extension
        .set_preferred_username("relay")?
        .set_followers(config.generate_url(UrlKind::Followers))?
        .set_following(config.generate_url(UrlKind::Following))?
        .set_inbox(config.generate_url(UrlKind::Inbox))?
        .set_outbox(config.generate_url(UrlKind::Outbox))?
        .set_endpoints(endpoint)?;

    let public_key = PublicKey {
        id: config.generate_url(UrlKind::MainKey).parse()?,
        owner: config.generate_url(UrlKind::Actor).parse()?,
        public_key_pem: state.public_key.to_pem_pkcs8()?,
    };

    Ok(ok(application.extend(public_key.to_ext())))
}
