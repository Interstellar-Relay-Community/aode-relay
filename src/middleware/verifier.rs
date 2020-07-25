use crate::{data::ActorCache, error::MyError, requests::Requests};
use activitystreams_new::uri;
use actix_web::web;
use http_signature_normalization_actix::{prelude::*, verify::DeprecatedAlgorithm};
use log::error;
use rsa::{hash::Hash, padding::PaddingScheme, PublicKey, RSAPublicKey};
use rsa_pem::KeyExt;
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin};

#[derive(Clone)]
pub struct MyVerify(pub Requests, pub ActorCache);

impl MyVerify {
    async fn verify(
        &self,
        algorithm: Option<Algorithm>,
        key_id: String,
        signature: String,
        signing_string: String,
    ) -> Result<bool, MyError> {
        let mut uri = uri!(key_id);
        uri.set_fragment(None);
        let actor = self.1.get(&uri, &self.0).await?;
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

        // Previously we verified the sig from an actor's local cache
        //
        // Now we make sure we fetch an updated actor
        let actor = self.1.get_no_cache(&uri, &self.0).await?;

        do_verify(&actor.public_key, signature, signing_string).await?;

        Ok(true)
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
