use crate::{
    config::{Config, UrlKind},
    data::State,
};
use actix_web::web::Data;
use actix_webfinger::{Resolver, Webfinger};
use rsa_magic_public_key::AsMagicPublicKey;
use std::{future::Future, pin::Pin};

pub struct RelayResolver;

#[derive(Clone, Debug, thiserror::Error)]
#[error("Error resolving webfinger data")]
pub struct RelayError;

type FutResult<T, E> = dyn Future<Output = Result<T, E>>;

impl Resolver for RelayResolver {
    type State = (Data<State>, Data<Config>);
    type Error = RelayError;

    fn find(
        account: &str,
        domain: &str,
        (state, config): Self::State,
    ) -> Pin<Box<FutResult<Option<Webfinger>, Self::Error>>> {
        let domain = domain.to_owned();
        let account = account.to_owned();

        let fut = async move {
            if domain != config.hostname() {
                return Ok(None);
            }

            if account != "relay" {
                return Ok(None);
            }

            let mut wf = Webfinger::new(&config.generate_resource());
            wf.add_alias(&config.generate_url(UrlKind::Actor))
                .add_activitypub(&config.generate_url(UrlKind::Actor))
                .add_magic_public_key(&state.public_key.as_magic_public_key());

            Ok(Some(wf))
        };

        Box::pin(fut)
    }
}

impl actix_web::error::ResponseError for RelayError {}
