use crate::{data::ActorCache, error::MyError, middleware::MyVerify, requests::Requests};
use config::Environment;
use http_signature_normalization_actix::prelude::{VerifyDigest, VerifySignature};
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
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
    connections_per_core: u32,
}

pub enum UrlKind {
    Activity,
    Actor,
    Followers,
    Following,
    Inbox,
    Index,
    MainKey,
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
            .set_default("connections_per_core", 2)?
            .merge(Environment::new())?;

        Ok(config.try_into()?)
    }

    pub fn pretty_log(&self) -> bool {
        self.pretty_log
    }

    pub fn connections_per_core(&self) -> u32 {
        self.connections_per_core
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

    pub fn generate_url(&self, kind: UrlKind) -> String {
        let scheme = if self.https { "https" } else { "http" };

        match kind {
            UrlKind::Activity => {
                format!("{}://{}/activity/{}", scheme, self.hostname, Uuid::new_v4())
            }
            UrlKind::Actor => format!("{}://{}/actor", scheme, self.hostname),
            UrlKind::Followers => format!("{}://{}/followers", scheme, self.hostname),
            UrlKind::Following => format!("{}://{}/following", scheme, self.hostname),
            UrlKind::Inbox => format!("{}://{}/inbox", scheme, self.hostname),
            UrlKind::Index => format!("{}://{}/", scheme, self.hostname),
            UrlKind::MainKey => format!("{}://{}/actor#main-key", scheme, self.hostname),
            UrlKind::NodeInfo => format!("{}://{}/nodeinfo/2.0.json", scheme, self.hostname),
            UrlKind::Outbox => format!("{}://{}/outbox", scheme, self.hostname),
        }
    }
}
