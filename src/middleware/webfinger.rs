use crate::{
    config::{Config, UrlKind},
    data::State,
};
use actix_web::web::Data;
use actix_webfinger::{Resolver, Webfinger};
use futures_util::future::LocalBoxFuture;
use rsa_magic_public_key::AsMagicPublicKey;

pub(crate) struct RelayResolver;

#[derive(Clone, Debug, thiserror::Error)]
#[error("Error resolving webfinger data")]
pub(crate) struct RelayError;

impl Resolver for RelayResolver {
    type State = (Data<State>, Data<Config>);
    type Error = RelayError;

    fn find(
        scheme: Option<&str>,
        account: &str,
        domain: &str,
        (state, config): Self::State,
    ) -> LocalBoxFuture<'static, Result<Option<Webfinger>, Self::Error>> {
        let domain = domain.to_owned();
        let account = account.to_owned();
        let scheme = scheme.map(|scheme| scheme.to_owned());

        let fut = async move {
            if let Some(scheme) = scheme {
                if scheme != "acct:" {
                    return Ok(None);
                }
            }

            if domain != config.hostname() {
                return Ok(None);
            }

            if account != "relay" {
                return Ok(None);
            }

            let mut wf = Webfinger::new(config.generate_resource().as_str());
            wf.add_alias(config.generate_url(UrlKind::Actor).as_str())
                .add_activitypub(config.generate_url(UrlKind::Actor).as_str())
                .add_magic_public_key(&state.public_key.as_magic_public_key());

            Ok(Some(wf))
        };

        Box::pin(fut)
    }
}

impl actix_web::error::ResponseError for RelayError {}
