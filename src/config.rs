use crate::{data::ActorCache, error::MyError, middleware::MyVerify, requests::Requests};
use activitystreams_new::primitives::XsdAnyUri;
use config::Environment;
use http_signature_normalization_actix::prelude::{VerifyDigest, VerifySignature};
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ParsedConfig {
    hostname: String,
    addr: IpAddr,
    port: u16,
    debug: bool,
    whitelist_mode: bool,
    validate_signatures: bool,
    https: bool,
    database_url: String,
    pretty_log: bool,
    publish_blocks: bool,
    max_connections: usize,
}

#[derive(Clone, Debug)]
pub struct Config {
    hostname: String,
    addr: IpAddr,
    port: u16,
    debug: bool,
    whitelist_mode: bool,
    validate_signatures: bool,
    database_url: String,
    pretty_log: bool,
    publish_blocks: bool,
    max_connections: usize,
    base_uri: XsdAnyUri,
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
    pub fn build() -> Result<Self, MyError> {
        let mut config = config::Config::new();
        config
            .set_default("hostname", "localhost:8080")?
            .set_default("addr", "127.0.0.1")?
            .set_default("port", 8080)?
            .set_default("debug", true)?
            .set_default("whitelist_mode", false)?
            .set_default("validate_signatures", false)?
            .set_default("https", false)?
            .set_default("pretty_log", true)?
            .set_default("publish_blocks", false)?
            .set_default("max_connections", 2)?
            .merge(Environment::new())?;

        let config: ParsedConfig = config.try_into()?;

        let scheme = if config.https { "https" } else { "http" };
        let base_uri = format!("{}://{}", scheme, config.hostname).parse()?;

        Ok(Config {
            hostname: config.hostname,
            addr: config.addr,
            port: config.port,
            debug: config.debug,
            whitelist_mode: config.whitelist_mode,
            validate_signatures: config.validate_signatures,
            database_url: config.database_url,
            pretty_log: config.pretty_log,
            publish_blocks: config.publish_blocks,
            max_connections: config.max_connections,
            base_uri,
        })
    }

    pub fn pretty_log(&self) -> bool {
        self.pretty_log
    }

    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    pub fn validate_signatures(&self) -> bool {
        self.validate_signatures
    }

    pub fn digest_middleware(&self) -> VerifyDigest<Sha256> {
        if self.validate_signatures {
            VerifyDigest::new(Sha256::new())
        } else {
            VerifyDigest::new(Sha256::new()).optional()
        }
    }

    pub fn signature_middleware(
        &self,
        requests: Requests,
        actors: ActorCache,
    ) -> VerifySignature<MyVerify> {
        if self.validate_signatures {
            VerifySignature::new(MyVerify(requests, actors), Default::default())
        } else {
            VerifySignature::new(MyVerify(requests, actors), Default::default()).optional()
        }
    }

    pub fn bind_address(&self) -> (IpAddr, u16) {
        (self.addr, self.port)
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn publish_blocks(&self) -> bool {
        self.publish_blocks
    }

    pub fn whitelist_mode(&self) -> bool {
        self.whitelist_mode
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub fn generate_resource(&self) -> String {
        format!("relay@{}", self.hostname)
    }

    pub fn software_name(&self) -> String {
        "AodeRelay".to_owned()
    }

    pub fn software_version(&self) -> String {
        "v0.1.0-master".to_owned()
    }

    pub fn source_code(&self) -> String {
        "https://git.asonix.dog/asonix/ap-relay".to_owned()
    }

    pub fn generate_url(&self, kind: UrlKind) -> XsdAnyUri {
        let mut uri = self.base_uri.clone();
        let url = uri.as_url_mut();

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

        uri
    }
}
