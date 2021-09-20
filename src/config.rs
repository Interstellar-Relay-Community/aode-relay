use crate::{
    data::{ActorCache, State},
    error::Error,
    middleware::MyVerify,
    requests::Requests,
};
use activitystreams::{uri, url::Url};
use config::Environment;
use http_signature_normalization_actix::prelude::{VerifyDigest, VerifySignature};
use sha2::{Digest, Sha256};
use std::{net::IpAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct ParsedConfig {
    hostname: String,
    addr: IpAddr,
    port: u16,
    debug: bool,
    restricted_mode: bool,
    validate_signatures: bool,
    https: bool,
    publish_blocks: bool,
    sled_path: PathBuf,
    source_repo: Url,
    opentelemetry_url: Option<Url>,
}

#[derive(Clone, Debug)]
pub struct Config {
    hostname: String,
    addr: IpAddr,
    port: u16,
    debug: bool,
    restricted_mode: bool,
    validate_signatures: bool,
    publish_blocks: bool,
    base_uri: Url,
    sled_path: PathBuf,
    source_repo: Url,
    opentelemetry_url: Option<Url>,
}

pub enum UrlKind {
    Activity,
    Actor,
    Followers,
    Following,
    Inbox,
    Index,
    MainKey,
    Media(Uuid),
    NodeInfo,
    Outbox,
}

impl Config {
    pub(crate) fn build() -> Result<Self, Error> {
        let mut config = config::Config::new();
        config
            .set_default("hostname", "localhost:8080")?
            .set_default("addr", "127.0.0.1")?
            .set_default("port", 8080)?
            .set_default("debug", true)?
            .set_default("restricted_mode", false)?
            .set_default("validate_signatures", false)?
            .set_default("https", false)?
            .set_default("publish_blocks", false)?
            .set_default("sled_path", "./sled/db-0-34")?
            .set_default("source_repo", "https://git.asonix.dog/asonix/relay")?
            .set_default("opentelemetry_url", None as Option<&str>)?
            .merge(Environment::new())?;

        let config: ParsedConfig = config.try_into()?;

        let scheme = if config.https { "https" } else { "http" };
        let base_uri = uri!(format!("{}://{}", scheme, config.hostname));

        Ok(Config {
            hostname: config.hostname,
            addr: config.addr,
            port: config.port,
            debug: config.debug,
            restricted_mode: config.restricted_mode,
            validate_signatures: config.validate_signatures,
            publish_blocks: config.publish_blocks,
            base_uri,
            sled_path: config.sled_path,
            source_repo: config.source_repo,
            opentelemetry_url: config.opentelemetry_url,
        })
    }

    pub(crate) fn sled_path(&self) -> &PathBuf {
        &self.sled_path
    }

    pub(crate) fn validate_signatures(&self) -> bool {
        self.validate_signatures
    }

    pub(crate) fn digest_middleware(&self) -> VerifyDigest<Sha256> {
        if self.validate_signatures {
            VerifyDigest::new(Sha256::new())
        } else {
            VerifyDigest::new(Sha256::new()).optional()
        }
    }

    pub(crate) fn signature_middleware(
        &self,
        requests: Requests,
        actors: ActorCache,
        state: State,
    ) -> VerifySignature<MyVerify> {
        if self.validate_signatures {
            VerifySignature::new(MyVerify(requests, actors, state), Default::default())
        } else {
            VerifySignature::new(MyVerify(requests, actors, state), Default::default()).optional()
        }
    }

    pub(crate) fn bind_address(&self) -> (IpAddr, u16) {
        (self.addr, self.port)
    }

    pub(crate) fn debug(&self) -> bool {
        self.debug
    }

    pub(crate) fn publish_blocks(&self) -> bool {
        self.publish_blocks
    }

    pub(crate) fn restricted_mode(&self) -> bool {
        self.restricted_mode
    }

    pub(crate) fn hostname(&self) -> &str {
        &self.hostname
    }

    pub(crate) fn generate_resource(&self) -> String {
        format!("relay@{}", self.hostname)
    }

    pub(crate) fn software_name() -> &'static str {
        "AodeRelay"
    }

    pub(crate) fn software_version() -> String {
        if let Some(git) = Self::git_version() {
            return format!("v{}-{}", Self::version(), git);
        }

        format!("v{}", Self::version())
    }

    fn git_version() -> Option<String> {
        let branch = Self::git_branch()?;
        let hash = Self::git_hash()?;

        Some(format!("{}-{}", branch, hash))
    }

    fn name() -> &'static str {
        env!("PKG_NAME")
    }

    fn version() -> &'static str {
        env!("PKG_VERSION")
    }

    fn git_branch() -> Option<&'static str> {
        option_env!("GIT_BRANCH")
    }

    fn git_hash() -> Option<&'static str> {
        option_env!("GIT_HASH")
    }

    pub(crate) fn user_agent(&self) -> String {
        format!(
            "{} ({}/{}; +{})",
            Self::software_name(),
            Self::name(),
            Self::software_version(),
            self.generate_url(UrlKind::Index),
        )
    }

    pub(crate) fn source_code(&self) -> &Url {
        &self.source_repo
    }

    pub(crate) fn opentelemetry_url(&self) -> Option<&Url> {
        self.opentelemetry_url.as_ref()
    }

    pub(crate) fn generate_url(&self, kind: UrlKind) -> Url {
        let mut url = self.base_uri.clone();

        match kind {
            UrlKind::Activity => url.set_path(&format!("activity/{}", Uuid::new_v4())),
            UrlKind::Actor => url.set_path("actor"),
            UrlKind::Followers => url.set_path("followers"),
            UrlKind::Following => url.set_path("following"),
            UrlKind::Inbox => url.set_path("inbox"),
            UrlKind::Index => (),
            UrlKind::MainKey => {
                url.set_path("actor");
                url.set_fragment(Some("main-key"));
            }
            UrlKind::Media(uuid) => url.set_path(&format!("media/{}", uuid)),
            UrlKind::NodeInfo => url.set_path("nodeinfo/2.0.json"),
            UrlKind::Outbox => url.set_path("outbox"),
        };

        url
    }
}
