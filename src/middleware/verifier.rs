use crate::{
    apub::AcceptedActors,
    data::{ActorCache, State},
    error::{Error, ErrorKind},
    requests::Requests,
};
use activitystreams::{base::BaseExt, iri, iri_string::types::IriString};
use actix_web::web;
use base64::{engine::general_purpose::STANDARD, Engine};
use http_signature_normalization_actix::{prelude::*, verify::DeprecatedAlgorithm};
use rsa::{
    pkcs1v15::Signature, pkcs1v15::VerifyingKey, pkcs8::DecodePublicKey, sha2::Sha256,
    signature::Verifier, RsaPublicKey,
};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug)]
pub(crate) struct MyVerify(pub Requests, pub ActorCache, pub State);

impl MyVerify {
    #[tracing::instrument("Verify request", skip(self, signature, signing_string))]
    async fn verify(
        &self,
        algorithm: Option<Algorithm>,
        key_id: String,
        signature: String,
        signing_string: String,
    ) -> Result<bool, Error> {
        let public_key_id = iri!(key_id);

        // receiving an activity from a domain indicates it is probably online
        self.0.reset_breaker(&public_key_id);

        let actor_id = if let Some(mut actor_id) = self
            .2
            .db
            .actor_id_from_public_key_id(public_key_id.clone())
            .await?
        {
            if !self.2.db.is_allowed(actor_id.clone()).await? {
                return Err(ErrorKind::NotAllowed(key_id).into());
            }

            actor_id.set_fragment(None);
            let actor = self.1.get(&actor_id, &self.0).await?;
            let was_cached = actor.is_cached();
            let actor = actor.into_inner();

            match algorithm {
                Some(Algorithm::Hs2019) => (),
                Some(Algorithm::Deprecated(DeprecatedAlgorithm::RsaSha256)) => (),
                Some(other) => {
                    return Err(ErrorKind::Algorithm(other.to_string()).into());
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
            match self.0.fetch::<PublicKeyResponse>(&public_key_id).await {
                Ok(res) => res.actor_id().ok_or(ErrorKind::MissingId),
                Err(e) => {
                    if e.is_gone() {
                        tracing::warn!("Actor gone: {public_key_id}");
                        return Ok(false);
                    } else {
                        return Err(e);
                    }
                }
            }?
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
        id: IriString,
        owner: IriString,
        #[allow(dead_code)]
        public_key_pem: String,
    },
    Actor(Box<AcceptedActors>),
}

impl PublicKeyResponse {
    fn actor_id(&self) -> Option<IriString> {
        match self {
            PublicKeyResponse::PublicKey { owner, .. } => Some(owner.clone()),
            PublicKeyResponse::Actor(actor) => actor.id_unchecked().cloned(),
        }
    }
}

#[tracing::instrument("Verify signature")]
async fn do_verify(
    public_key: &str,
    signature: String,
    signing_string: String,
) -> Result<(), Error> {
    let public_key = RsaPublicKey::from_public_key_pem(public_key.trim())?;

    let span = tracing::Span::current();
    web::block(move || {
        span.in_scope(|| {
            let decoded = STANDARD.decode(signature)?;
            let signature =
                Signature::try_from(decoded.as_slice()).map_err(ErrorKind::ReadSignature)?;

            let verifying_key = VerifyingKey::<Sha256>::new(public_key);
            verifying_key
                .verify(signing_string.as_bytes(), &signature)
                .map_err(ErrorKind::VerifySignature)?;

            Ok(()) as Result<(), Error>
        })
    })
    .await??;

    Ok(())
}

impl SignatureVerify for MyVerify {
    type Error = Error;
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
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::apub::AcceptedActors;
    use rsa::{pkcs8::DecodePublicKey, RsaPublicKey};

    const ASONIX_DOG_ACTOR: &str = r#"{"@context":["https://www.w3.org/ns/activitystreams","https://w3id.org/security/v1",{"manuallyApprovesFollowers":"as:manuallyApprovesFollowers","toot":"http://joinmastodon.org/ns#","featured":{"@id":"toot:featured","@type":"@id"},"featuredTags":{"@id":"toot:featuredTags","@type":"@id"},"alsoKnownAs":{"@id":"as:alsoKnownAs","@type":"@id"},"movedTo":{"@id":"as:movedTo","@type":"@id"},"schema":"http://schema.org#","PropertyValue":"schema:PropertyValue","value":"schema:value","discoverable":"toot:discoverable","Device":"toot:Device","Ed25519Signature":"toot:Ed25519Signature","Ed25519Key":"toot:Ed25519Key","Curve25519Key":"toot:Curve25519Key","EncryptedMessage":"toot:EncryptedMessage","publicKeyBase64":"toot:publicKeyBase64","deviceId":"toot:deviceId","claim":{"@type":"@id","@id":"toot:claim"},"fingerprintKey":{"@type":"@id","@id":"toot:fingerprintKey"},"identityKey":{"@type":"@id","@id":"toot:identityKey"},"devices":{"@type":"@id","@id":"toot:devices"},"messageFranking":"toot:messageFranking","messageType":"toot:messageType","cipherText":"toot:cipherText","suspended":"toot:suspended"}],"id":"https://masto.asonix.dog/actor","type":"Application","inbox":"https://masto.asonix.dog/actor/inbox","outbox":"https://masto.asonix.dog/actor/outbox","preferredUsername":"masto.asonix.dog","url":"https://masto.asonix.dog/about/more?instance_actor=true","manuallyApprovesFollowers":true,"publicKey":{"id":"https://masto.asonix.dog/actor#main-key","owner":"https://masto.asonix.dog/actor","publicKeyPem":"-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAx8zXS0QNg9YGUBsxAOBH\nJaxIn7i6t+Z4UOpSFDVa2kP0NvQgIJsq3wzRqvaiuncRWpkyFk1fTakiRGD32xnY\nt+juuAaIBlU8eswKyANFqhcLAvFHmT3rA1848M4/YM19djvlL/PR9T53tPNHU+el\nS9MlsG3o6Zsj8YaUJtCI8RgEuJoROLHUb/V9a3oMQ7CfuIoSvF3VEz3/dRT09RW6\n0wQX7yhka9WlKuayWLWmTcB9lAIX6neBk+qKc8VSEsO7mHkzB8mRgVcS2uYZl1eA\nD8/jTT+SlpeFNDZk0Oh35GNFoOxh9qjRw3NGxu7jJCVBehDODzasOv4xDxKAhONa\njQIDAQAB\n-----END PUBLIC KEY-----\n"},"endpoints":{"sharedInbox":"https://masto.asonix.dog/inbox"}}"#;
    const KARJALAZET_RELAY: &str = r#"{"@context":["https://www.w3.org/ns/activitystreams","https://pleroma.karjalazet.se/schemas/litepub-0.1.jsonld",{"@language":"und"}],"alsoKnownAs":[],"attachment":[],"capabilities":{},"discoverable":false,"endpoints":{"oauthAuthorizationEndpoint":"https://pleroma.karjalazet.se/oauth/authorize","oauthRegistrationEndpoint":"https://pleroma.karjalazet.se/api/v1/apps","oauthTokenEndpoint":"https://pleroma.karjalazet.se/oauth/token","sharedInbox":"https://pleroma.karjalazet.se/inbox","uploadMedia":"https://pleroma.karjalazet.se/api/ap/upload_media"},"featured":"https://pleroma.karjalazet.se/relay/collections/featured","followers":"https://pleroma.karjalazet.se/relay/followers","following":"https://pleroma.karjalazet.se/relay/following","id":"https://pleroma.karjalazet.se/relay","inbox":"https://pleroma.karjalazet.se/relay/inbox","manuallyApprovesFollowers":false,"name":null,"outbox":"https://pleroma.karjalazet.se/relay/outbox","preferredUsername":"relay","publicKey":{"id":"https://pleroma.karjalazet.se/relay#main-key","owner":"https://pleroma.karjalazet.se/relay","publicKeyPem":"-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAucoyCht6QpEzUPdQWP/J\nJYxObSH3MCcXBnG4d0OX78QshloeAHhl78EZ5c8I0ePmIjDg2NFK3/pG0EvSrHe2\nIZHnHaN5emgCb2ifNya5W572yfQXo1tUQy+ZXtbTUA7BWbr4LuCvd+HUavMwbx72\neraSZTiQj//ObwpbXFoZO5I/+e5avGmVnfmr/y2cG95hqFDtI3438RgZyBjY5kJM\nY1MLWoY9itGSfYmBtxRj3umlC2bPuBB+hHUJi6TvP7NO6zuUZ66m4ETyuBDi8iP6\ngnUp3Q4+1/I3nDUmhjt7OXckUcX3r5M4UHD3VVUFG0aZw6WWMEAxlyFf/07fCkhR\nBwIDAQAB\n-----END PUBLIC KEY-----\n\n"},"summary":"","tag":[],"type":"Person","url":"https://pleroma.karjalazet.se/relay"}"#;
    const ASONIX_DOG_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAx8zXS0QNg9YGUBsxAOBH\nJaxIn7i6t+Z4UOpSFDVa2kP0NvQgIJsq3wzRqvaiuncRWpkyFk1fTakiRGD32xnY\nt+juuAaIBlU8eswKyANFqhcLAvFHmT3rA1848M4/YM19djvlL/PR9T53tPNHU+el\nS9MlsG3o6Zsj8YaUJtCI8RgEuJoROLHUb/V9a3oMQ7CfuIoSvF3VEz3/dRT09RW6\n0wQX7yhka9WlKuayWLWmTcB9lAIX6neBk+qKc8VSEsO7mHkzB8mRgVcS2uYZl1eA\nD8/jTT+SlpeFNDZk0Oh35GNFoOxh9qjRw3NGxu7jJCVBehDODzasOv4xDxKAhONa\njQIDAQAB\n-----END PUBLIC KEY-----\n";
    const KARJALAZET_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAucoyCht6QpEzUPdQWP/J\nJYxObSH3MCcXBnG4d0OX78QshloeAHhl78EZ5c8I0ePmIjDg2NFK3/pG0EvSrHe2\nIZHnHaN5emgCb2ifNya5W572yfQXo1tUQy+ZXtbTUA7BWbr4LuCvd+HUavMwbx72\neraSZTiQj//ObwpbXFoZO5I/+e5avGmVnfmr/y2cG95hqFDtI3438RgZyBjY5kJM\nY1MLWoY9itGSfYmBtxRj3umlC2bPuBB+hHUJi6TvP7NO6zuUZ66m4ETyuBDi8iP6\ngnUp3Q4+1/I3nDUmhjt7OXckUcX3r5M4UHD3VVUFG0aZw6WWMEAxlyFf/07fCkhR\nBwIDAQAB\n-----END PUBLIC KEY-----\n\n";

    #[test]
    fn handles_masto_keys() {
        println!("{ASONIX_DOG_KEY}");
        let _ = RsaPublicKey::from_public_key_pem(ASONIX_DOG_KEY.trim()).unwrap();
    }

    #[test]
    fn handles_pleromo_keys() {
        println!("{KARJALAZET_KEY}");
        let _ = RsaPublicKey::from_public_key_pem(KARJALAZET_KEY.trim()).unwrap();
    }

    #[test]
    fn handles_pleromo_relay_format() {
        let _: AcceptedActors = serde_json::from_str(KARJALAZET_RELAY).unwrap();
    }

    #[test]
    fn handles_masto_relay_format() {
        let _: AcceptedActors = serde_json::from_str(ASONIX_DOG_ACTOR).unwrap();
    }
}
