use crate::{data::ActorCache, error::MyError, requests::Requests};
use activitystreams::primitives::XsdAnyUri;
use http_signature_normalization_actix::{prelude::*, verify::DeprecatedAlgorithm};
use log::{error, warn};
use rsa::{hash::Hashes, padding::PaddingScheme, PublicKey, RSAPublicKey};
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
        let mut uri: XsdAnyUri = key_id.parse()?;
        uri.as_url_mut().set_fragment(None);
        let actor = self.1.get(&uri, &self.0).await?;

        let public_key = RSAPublicKey::from_pem_pkcs8(&actor.public_key)?;

        match algorithm {
            Some(Algorithm::Hs2019) => (),
            Some(Algorithm::Deprecated(DeprecatedAlgorithm::RsaSha256)) => (),
            other => {
                warn!("Invalid algorithm supplied for signature, {:?}", other);
                return Err(MyError::Algorithm);
            }
        };

        let decoded = base64::decode(signature)?;
        let hashed = Sha256::digest(signing_string.as_bytes());

        public_key.verify(
            PaddingScheme::PKCS1v15,
            Some(&Hashes::SHA2_256),
            &hashed,
            &decoded,
        )?;

        Ok(true)
    }
}

impl SignatureVerify for MyVerify {
    type Error = MyError;
    type Future = Pin<Box<dyn Future<Output = Result<bool, Self::Error>>>>;

    fn signature_verify(
        &mut self,
        algorithm: Option<Algorithm>,
        key_id: &str,
        signature: &str,
        signing_string: &str,
    ) -> Self::Future {
        let key_id = key_id.to_owned();
        let signature = signature.to_owned();
        let signing_string = signing_string.to_owned();

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
