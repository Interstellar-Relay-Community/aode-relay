use crate::{
    apub::AcceptedActors,
    data::{ActorCache, State},
    error::MyError,
    requests::Requests,
};
use activitystreams::{base::BaseExt, uri, url::Url};
use actix_web::web;
use http_signature_normalization_actix::{prelude::*, verify::DeprecatedAlgorithm};
use log::error;
use rsa::{hash::Hash, padding::PaddingScheme, PublicKey, RSAPublicKey};
use rsa_pem::KeyExt;
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin};

#[derive(Clone)]
pub(crate) struct MyVerify(pub Requests, pub ActorCache, pub State);

impl MyVerify {
    async fn verify(
        &self,
        algorithm: Option<Algorithm>,
        key_id: String,
        signature: String,
        signing_string: String,
    ) -> Result<bool, MyError> {
        let public_key_id = uri!(key_id);

        let actor_id = if let Some(mut actor_id) = self
            .2
            .db
            .actor_id_from_public_key_id(public_key_id.clone())
            .await?
        {
            if !self.2.db.is_allowed(actor_id.clone()).await? {
                return Err(MyError::NotAllowed(key_id));
            }

            actor_id.set_fragment(None);
            let actor = self.1.get(&actor_id, &self.0).await?;
            let was_cached = actor.is_cached();
            let actor = actor.into_inner();

            match algorithm {
                Some(Algorithm::Hs2019) => (),
                Some(Algorithm::Deprecated(DeprecatedAlgorithm::RsaSha256)) => (),
                Some(other) => {
                    return Err(MyError::Algorithm(other.to_string()));
                }
                None => (),
            };

            let res = do_verify(&actor.public_key, signature.clone(), signing_string.clone()).await;

            if let Err(e) = res {
                if !was_cached {
                    return Err(e);
                }
            } else {
                return Ok(true);
            }

            actor_id
        } else {
            self.0
                .fetch::<PublicKeyResponse>(public_key_id.as_str())
                .await?
                .actor_id()
                .ok_or_else(|| MyError::MissingId)?
        };

        // Previously we verified the sig from an actor's local cache
        //
        // Now we make sure we fetch an updated actor
        let actor = self.1.get_no_cache(&actor_id, &self.0).await?;

        do_verify(&actor.public_key, signature, signing_string).await?;

        Ok(true)
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
#[serde(rename_all = "camelCase")]
enum PublicKeyResponse {
    PublicKey {
        #[allow(dead_code)]
        id: Url,
        owner: Url,
        #[allow(dead_code)]
        public_key_pem: String,
    },
    Actor(AcceptedActors),
}

impl PublicKeyResponse {
    fn actor_id(&self) -> Option<Url> {
        match self {
            PublicKeyResponse::PublicKey { owner, .. } => Some(owner.clone()),
            PublicKeyResponse::Actor(actor) => actor.id_unchecked().map(|url| url.clone()),
        }
    }
}

async fn do_verify(
    public_key: &str,
    signature: String,
    signing_string: String,
) -> Result<(), MyError> {
    let public_key = RSAPublicKey::from_pem_pkcs8(public_key)?;

    web::block(move || {
        let decoded = base64::decode(signature)?;
        let hashed = Sha256::digest(signing_string.as_bytes());

        public_key.verify(
            PaddingScheme::PKCS1v15Sign {
                hash: Some(Hash::SHA2_256),
            },
            &hashed,
            &decoded,
        )?;

        Ok(()) as Result<(), MyError>
    })
    .await?;

    Ok(())
}

impl SignatureVerify for MyVerify {
    type Error = MyError;
    type Future = Pin<Box<dyn Future<Output = Result<bool, Self::Error>>>>;

    fn signature_verify(
        &mut self,
        algorithm: Option<Algorithm>,
        key_id: String,
        signature: String,
        signing_string: String,
    ) -> Self::Future {
        let this = self.clone();

        Box::pin(async move {
            this.verify(algorithm, key_id, signature, signing_string)
                .await
                .map_err(|e| {
                    error!("Failed to verify, {}", e);
                    e
                })
        })
    }
}
