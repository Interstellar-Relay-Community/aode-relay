use crate::{error::MyError, requests::fetch_actor, state::State};
use actix_web::client::Client;
use http_signature_normalization_actix::{prelude::*, verify::DeprecatedAlgorithm};
use rsa::{hash::Hashes, padding::PaddingScheme, PublicKey, RSAPublicKey};
use rsa_pem::KeyExt;
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin, sync::Arc};

#[derive(Clone)]
pub struct MyVerify(pub State, pub Client);

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

        let state = Arc::new(self.0.clone());
        let client = Arc::new(self.1.clone());

        Box::pin(async move {
            let actor = fetch_actor(state, client, &key_id.parse()?).await?;

            let public_key = actor.public_key.ok_or(MyError::MissingKey)?;

            let public_key = RSAPublicKey::from_pem_pkcs8(&public_key.public_key_pem)?;

            match algorithm {
                Some(Algorithm::Hs2019) => (),
                Some(Algorithm::Deprecated(DeprecatedAlgorithm::RsaSha256)) => (),
                _ => return Err(MyError::Algorithm),
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
        })
    }
}
