use crate::{
    data::LastOnline,
    error::{Error, ErrorKind},
    spawner::Spawner,
};
use activitystreams::iri_string::types::IriString;
use actix_web::http::header::Date;
use base64::{engine::general_purpose::STANDARD, Engine};
use dashmap::DashMap;
use http_signature_normalization_reqwest::{digest::ring::Sha256, prelude::*};
use reqwest_middleware::ClientWithMiddleware;
use ring::{
    rand::SystemRandom,
    signature::{RsaKeyPair, RSA_PKCS1_SHA256},
};
use rsa::{pkcs1::EncodeRsaPrivateKey, RsaPrivateKey};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

const ONE_SECOND: u64 = 1;
const ONE_MINUTE: u64 = 60 * ONE_SECOND;
const ONE_HOUR: u64 = 60 * ONE_MINUTE;
const ONE_DAY: u64 = 24 * ONE_HOUR;

#[derive(Debug)]
pub(crate) enum BreakerStrategy {
    // Requires a successful response
    Require2XX,
    // Allows HTTP 2xx-401
    Allow401AndBelow,
    // Allows HTTP 2xx-404
    Allow404AndBelow,
}

#[derive(Clone)]
pub(crate) struct Breakers {
    inner: Arc<DashMap<String, Breaker>>,
}

impl std::fmt::Debug for Breakers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Breakers").finish()
    }
}

impl Breakers {
    pub(crate) fn should_try(&self, url: &IriString) -> bool {
        if let Some(authority) = url.authority_str() {
            if let Some(breaker) = self.inner.get(authority) {
                breaker.should_try()
            } else {
                true
            }
        } else {
            false
        }
    }

    fn fail(&self, url: &IriString) {
        if let Some(authority) = url.authority_str() {
            let should_write = {
                if let Some(mut breaker) = self.inner.get_mut(authority) {
                    breaker.fail();
                    if !breaker.should_try() {
                        tracing::warn!("Failed breaker for {authority}");
                    }
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut breaker = self.inner.entry(authority.to_owned()).or_default();
                breaker.fail();
            }
        }
    }

    fn succeed(&self, url: &IriString) {
        if let Some(authority) = url.authority_str() {
            let should_write = {
                if let Some(mut breaker) = self.inner.get_mut(authority) {
                    breaker.succeed();
                    false
                } else {
                    true
                }
            };

            if should_write {
                let mut breaker = self.inner.entry(authority.to_owned()).or_default();
                breaker.succeed();
            }
        }
    }
}

impl Default for Breakers {
    fn default() -> Self {
        Breakers {
            inner: Arc::new(DashMap::new()),
        }
    }
}

#[derive(Debug)]
struct Breaker {
    failures: usize,
    last_attempt: SystemTime,
    last_success: SystemTime,
}

impl Breaker {
    const FAILURE_WAIT: Duration = Duration::from_secs(ONE_DAY);
    const FAILURE_THRESHOLD: usize = 10;

    fn should_try(&self) -> bool {
        self.failures < Self::FAILURE_THRESHOLD
            || self.last_attempt + Self::FAILURE_WAIT < SystemTime::now()
    }

    fn fail(&mut self) {
        self.failures += 1;
        self.last_attempt = SystemTime::now();
    }

    fn succeed(&mut self) {
        self.failures = 0;
        self.last_attempt = SystemTime::now();
        self.last_success = SystemTime::now();
    }
}

impl Default for Breaker {
    fn default() -> Self {
        let now = SystemTime::now();

        Breaker {
            failures: 0,
            last_attempt: now,
            last_success: now,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Requests {
    client: ClientWithMiddleware,
    key_id: String,
    private_key: Arc<RsaKeyPair>,
    rng: SystemRandom,
    config: Config<Spawner>,
    breakers: Breakers,
    last_online: Arc<LastOnline>,
}

impl std::fmt::Debug for Requests {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Requests")
            .field("key_id", &self.key_id)
            .field("config", &self.config)
            .field("breakers", &self.breakers)
            .finish()
    }
}

impl Requests {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        key_id: String,
        private_key: RsaPrivateKey,
        breakers: Breakers,
        last_online: Arc<LastOnline>,
        spawner: Spawner,
        client: ClientWithMiddleware,
    ) -> Self {
        let private_key_der = private_key.to_pkcs1_der().expect("Can encode der");
        let private_key = ring::signature::RsaKeyPair::from_der(private_key_der.as_bytes())
            .expect("Key is valid");
        Requests {
            client,
            key_id,
            private_key: Arc::new(private_key),
            rng: SystemRandom::new(),
            config: Config::new_with_spawner(spawner).mastodon_compat(),
            breakers,
            last_online,
        }
    }

    pub(crate) fn spawner(mut self, spawner: Spawner) -> Self {
        self.config = self.config.set_spawner(spawner);
        self
    }

    pub(crate) fn reset_breaker(&self, iri: &IriString) {
        self.breakers.succeed(iri);
    }

    async fn check_response(
        &self,
        parsed_url: &IriString,
        strategy: BreakerStrategy,
        res: Result<reqwest::Response, reqwest_middleware::Error>,
    ) -> Result<reqwest::Response, Error> {
        if res.is_err() {
            self.breakers.fail(&parsed_url);
        }

        let res = res?;

        let status = res.status();

        let success = match strategy {
            BreakerStrategy::Require2XX => status.is_success(),
            BreakerStrategy::Allow401AndBelow => (200..=401).contains(&status.as_u16()),
            BreakerStrategy::Allow404AndBelow => (200..=404).contains(&status.as_u16()),
        };

        if !success {
            self.breakers.fail(&parsed_url);

            if let Ok(s) = res.text().await {
                if !s.is_empty() {
                    tracing::debug!("Response from {parsed_url}, {s}");
                }
            }

            return Err(ErrorKind::Status(parsed_url.to_string(), status).into());
        }

        // only actually succeed a breaker on 2xx response
        if status.is_success() {
            self.last_online.mark_seen(&parsed_url);
            self.breakers.succeed(&parsed_url);
        }

        Ok(res)
    }

    #[tracing::instrument(name = "Fetch Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_json<T>(
        &self,
        url: &IriString,
        strategy: BreakerStrategy,
    ) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/json", strategy).await
    }

    #[tracing::instrument(name = "Fetch Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_json_msky<T>(
        &self,
        url: &IriString,
        strategy: BreakerStrategy,
    ) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let body = self
            .do_deliver(
                url,
                &serde_json::json!({}),
                "application/json",
                "application/json",
                strategy,
            )
            .await?
            .bytes()
            .await?;

        Ok(serde_json::from_slice(&body)?)
    }

    #[tracing::instrument(name = "Fetch Activity+Json", skip(self), fields(signing_string))]
    pub(crate) async fn fetch<T>(
        &self,
        url: &IriString,
        strategy: BreakerStrategy,
    ) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.do_fetch(url, "application/activity+json", strategy)
            .await
    }

    async fn do_fetch<T>(
        &self,
        url: &IriString,
        accept: &str,
        strategy: BreakerStrategy,
    ) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let body = self
            .do_fetch_response(url, accept, strategy)
            .await?
            .bytes()
            .await?;

        Ok(serde_json::from_slice(&body)?)
    }

    #[tracing::instrument(name = "Fetch response", skip(self), fields(signing_string))]
    pub(crate) async fn fetch_response(
        &self,
        url: &IriString,
        strategy: BreakerStrategy,
    ) -> Result<reqwest::Response, Error> {
        self.do_fetch_response(url, "*/*", strategy).await
    }

    pub(crate) async fn do_fetch_response(
        &self,
        url: &IriString,
        accept: &str,
        strategy: BreakerStrategy,
    ) -> Result<reqwest::Response, Error> {
        if !self.breakers.should_try(url) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
        let span = tracing::Span::current();

        let request = self
            .client
            .get(url.as_str())
            .header("Accept", accept)
            .header("Date", Date(SystemTime::now().into()).to_string())
            .signature(&self.config, self.key_id.clone(), move |signing_string| {
                span.record("signing_string", signing_string);
                span.in_scope(|| signer.sign(signing_string))
            })
            .await?;

        let res = self.client.execute(request).await;

        let res = self.check_response(url, strategy, res).await?;

        Ok(res)
    }

    #[tracing::instrument(
        "Deliver to Inbox",
        skip_all,
        fields(inbox = inbox.to_string().as_str(), signing_string)
    )]
    pub(crate) async fn deliver<T>(
        &self,
        inbox: &IriString,
        item: &T,
        strategy: BreakerStrategy,
    ) -> Result<(), Error>
    where
        T: serde::ser::Serialize + std::fmt::Debug,
    {
        self.do_deliver(
            inbox,
            item,
            "application/activity+json",
            "application/activity+json",
            strategy,
        )
        .await?;
        Ok(())
    }

    async fn do_deliver<T>(
        &self,
        inbox: &IriString,
        item: &T,
        content_type: &str,
        accept: &str,
        strategy: BreakerStrategy,
    ) -> Result<reqwest::Response, Error>
    where
        T: serde::ser::Serialize + std::fmt::Debug,
    {
        if !self.breakers.should_try(&inbox) {
            return Err(ErrorKind::Breaker.into());
        }

        let signer = self.signer();
        let span = tracing::Span::current();
        let item_string = serde_json::to_string(item)?;

        let request = self
            .client
            .post(inbox.as_str())
            .header("Accept", accept)
            .header("Content-Type", content_type)
            .header("Date", Date(SystemTime::now().into()).to_string())
            .signature_with_digest(
                self.config.clone(),
                self.key_id.clone(),
                Sha256::new(),
                item_string,
                move |signing_string| {
                    span.record("signing_string", signing_string);
                    span.in_scope(|| signer.sign(signing_string))
                },
            )
            .await?;

        let res = self.client.execute(request).await;

        let res = self.check_response(inbox, strategy, res).await?;

        Ok(res)
    }

    fn signer(&self) -> Signer {
        Signer {
            private_key: self.private_key.clone(),
            rng: self.rng.clone(),
        }
    }
}

struct Signer {
    private_key: Arc<RsaKeyPair>,
    rng: SystemRandom,
}

impl Signer {
    fn sign(&self, signing_string: &str) -> Result<String, Error> {
        let mut signature = vec![0; self.private_key.public_modulus_len()];

        self.private_key
            .sign(
                &RSA_PKCS1_SHA256,
                &self.rng,
                signing_string.as_bytes(),
                &mut signature,
            )
            .map_err(|_| ErrorKind::SignRequest)?;

        Ok(STANDARD.encode(&signature))
    }
}
