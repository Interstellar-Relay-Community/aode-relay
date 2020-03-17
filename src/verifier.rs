use crate::{error::MyError, requests::fetch_actor, state::State};
use actix_web::client::Client;
use http_signature_normalization_actix::{prelude::*, verify::DeprecatedAlgorithm};
use log::{debug, error, warn};
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
            verify(state, client, algorithm, key_id, signature, signing_string)
                .await
                .map_err(|e| {
                    error!("Failed to verify, {}", e);
                    e
                })
        })
    }
}

async fn verify(
    state: Arc<State>,
    client: Arc<Client>,
    algorithm: Option<Algorithm>,
    key_id: String,
    signature: String,
    signing_string: String,
) -> Result<bool, MyError> {
    debug!("Fetching actor");
    let actor = fetch_actor(state, client, &key_id.parse()?).await?;

    debug!("Parsing public key");
    let public_key = RSAPublicKey::from_pem_pkcs8(&actor.public_key.public_key_pem)?;

    match algorithm {
        Some(Algorithm::Hs2019) => (),
        Some(Algorithm::Deprecated(DeprecatedAlgorithm::RsaSha256)) => (),
        other => {
            warn!("Invalid algorithm supplied for signature, {:?}", other);
            return Err(MyError::Algorithm);
        }
    };

    debug!("Decoding base64");
    let decoded = base64::decode(signature)?;
    debug!("hashing");
    let hashed = Sha256::digest(signing_string.as_bytes());

    debug!("Verifying signature for {}", key_id);
    public_key.verify(
        PaddingScheme::PKCS1v15,
        Some(&Hashes::SHA2_256),
        &hashed,
        &decoded,
    )?;

    Ok(true)
}
