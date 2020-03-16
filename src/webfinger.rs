use crate::state::{State, UrlKind};
use activitystreams::context;
use actix_web::web::Data;
use actix_webfinger::{Link, Resolver, Webfinger};
use rsa_magic_public_key::AsMagicPublicKey;
use std::{future::Future, pin::Pin};

pub struct RelayResolver;

#[derive(Clone, Debug, thiserror::Error)]
#[error("Error resolving webfinger data")]
pub struct RelayError;

impl Resolver<Data<State>> for RelayResolver {
    type Error = RelayError;

    fn find(
        account: &str,
        domain: &str,
        state: Data<State>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Webfinger>, Self::Error>>>> {
        let domain = domain.to_owned();
        let account = account.to_owned();

        let fut = async move {
            if domain != state.settings.hostname {
                return Ok(None);
            }

            if account != "relay" {
                return Ok(None);
            }

            let mut wf = Webfinger::new(&state.generate_resource());
            wf.add_alias(&state.generate_url(UrlKind::Actor))
                .add_activitypub(&state.generate_url(UrlKind::Actor))
                .add_magic_public_key(&state.settings.public_key.as_magic_public_key())
                .add_link(Link {
                    rel: "self".to_owned(),
                    href: Some(state.generate_url(UrlKind::Actor)),
                    template: None,
                    kind: Some(format!("application/ld+json; profile=\"{}\"", context())),
                });

            Ok(Some(wf))
        };

        Box::pin(fut)
    }
}

impl actix_web::error::ResponseError for RelayError {}
