use crate::{
    error::Error,
    extractors::{AdminConfig, XApiToken},
};
use activitystreams::{
    iri,
    iri_string::{
        format::ToDedicatedString,
        resolve::FixedBaseResolver,
        types::{IriAbsoluteString, IriFragmentStr, IriRelativeStr, IriString},
    },
};
use config::Environment;
use http_signature_normalization_actix::prelude::VerifyDigest;
use rsa::sha2::{Digest, Sha256};
use rustls::{Certificate, PrivateKey};
use std::{
    io::BufReader,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
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
    source_repo: IriString,
    repository_commit_base: String,
    opentelemetry_url: Option<IriString>,
    telegram_token: Option<String>,
    telegram_admin_handle: Option<String>,
    api_token: Option<String>,
    tls_key: Option<PathBuf>,
    tls_cert: Option<PathBuf>,
    footer_blurb: Option<String>,
    local_domains: Option<String>,
    local_blurb: Option<String>,
    prometheus_addr: Option<IpAddr>,
    prometheus_port: Option<u16>,
    client_pool_size: usize,
}

#[derive(Clone)]
pub struct Config {
    hostname: String,
    addr: IpAddr,
    port: u16,
    debug: bool,
    restricted_mode: bool,
    validate_signatures: bool,
    publish_blocks: bool,
    base_uri: IriAbsoluteString,
    sled_path: PathBuf,
    source_repo: IriString,
    opentelemetry_url: Option<IriString>,
    telegram_token: Option<String>,
    telegram_admin_handle: Option<String>,
    api_token: Option<String>,
    tls: Option<TlsConfig>,
    footer_blurb: Option<String>,
    local_domains: Vec<String>,
    local_blurb: Option<String>,
    prometheus_config: Option<PrometheusConfig>,
    client_pool_size: usize,
}

#[derive(Clone)]
struct TlsConfig {
    key: PathBuf,
    cert: PathBuf,
}

#[derive(Clone, Debug)]
struct PrometheusConfig {
    addr: IpAddr,
    port: u16,
}

#[derive(Debug)]
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

#[derive(Debug)]
pub enum AdminUrlKind {
    Allow,
    Disallow,
    Block,
    Unblock,
    Allowed,
    Blocked,
    Connected,
    Stats,
    LastSeen,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("hostname", &self.hostname)
            .field("addr", &self.addr)
            .field("port", &self.port)
            .field("debug", &self.debug)
            .field("restricted_mode", &self.restricted_mode)
            .field("validate_signatures", &self.validate_signatures)
            .field("publish_blocks", &self.publish_blocks)
            .field("base_uri", &self.base_uri.to_string())
            .field("sled_path", &self.sled_path)
            .field("source_repo", &self.source_repo.to_string())
            .field(
                "opentelemetry_url",
                &self.opentelemetry_url.as_ref().map(|url| url.to_string()),
            )
            .field("telegram_token", &"[redacted]")
            .field("telegram_admin_handle", &self.telegram_admin_handle)
            .field("api_token", &"[redacted]")
            .field("tls_key", &"[redacted]")
            .field("tls_cert", &"[redacted]")
            .field("footer_blurb", &self.footer_blurb)
            .field("local_domains", &self.local_domains)
            .field("local_blurb", &self.local_blurb)
            .field("prometheus_config", &self.prometheus_config)
            .field("client_pool_size", &self.client_pool_size)
            .finish()
    }
}

impl Config {
    pub(crate) fn build() -> Result<Self, Error> {
        let config = config::Config::builder()
            .set_default("hostname", "localhost:8080")?
            .set_default("addr", "127.0.0.1")?
            .set_default("port", 8080u64)?
            .set_default("debug", true)?
            .set_default("restricted_mode", false)?
            .set_default("validate_signatures", true)?
            .set_default("https", true)?
            .set_default("publish_blocks", false)?
            .set_default("sled_path", "./sled/db-0-34")?
            .set_default("source_repo", "https://git.asonix.dog/asonix/relay")?
            .set_default("repository_commit_base", "/src/commit/")?
            .set_default("opentelemetry_url", None as Option<&str>)?
            .set_default("telegram_token", None as Option<&str>)?
            .set_default("telegram_admin_handle", None as Option<&str>)?
            .set_default("api_token", None as Option<&str>)?
            .set_default("tls_key", None as Option<&str>)?
            .set_default("tls_cert", None as Option<&str>)?
            .set_default("footer_blurb", None as Option<&str>)?
            .set_default("local_domains", None as Option<&str>)?
            .set_default("local_blurb", None as Option<&str>)?
            .set_default("prometheus_addr", None as Option<&str>)?
            .set_default("prometheus_port", None as Option<u16>)?
            .set_default("client_pool_size", 20u64)?
            .add_source(Environment::default())
            .build()?;

        let config: ParsedConfig = config.try_deserialize()?;

        let scheme = if config.https { "https" } else { "http" };
        let base_uri = iri!(format!("{scheme}://{}", config.hostname)).into_absolute();

        let tls = match (config.tls_key, config.tls_cert) {
            (Some(key), Some(cert)) => Some(TlsConfig { key, cert }),
            (Some(_), None) => {
                tracing::warn!("TLS_KEY is set but TLS_CERT isn't , not building TLS config");
                None
            }
            (None, Some(_)) => {
                tracing::warn!("TLS_CERT is set but TLS_KEY isn't , not building TLS config");
                None
            }
            (None, None) => None,
        };

        let local_domains = config
            .local_domains
            .iter()
            .flat_map(|s| s.split(','))
            .map(|d| d.to_string())
            .collect();

        let prometheus_config = match (config.prometheus_addr, config.prometheus_port) {
            (Some(addr), Some(port)) => Some(PrometheusConfig { addr, port }),
            (Some(_), None) => {
                tracing::warn!("PROMETHEUS_ADDR is set but PROMETHEUS_PORT is not set, not building Prometheus config");
                None
            }
            (None, Some(_)) => {
                tracing::warn!("PROMETHEUS_PORT is set but PROMETHEUS_ADDR is not set, not building Prometheus config");
                None
            }
            (None, None) => None,
        };

        let source_url = match Self::git_hash() {
            Some(hash) => format!(
                "{}{}{hash}",
                config.source_repo, config.repository_commit_base
            )
            .parse()
            .expect("constructed source URL is valid"),
            None => config.source_repo.clone(),
        };

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
            source_repo: source_url,
            opentelemetry_url: config.opentelemetry_url,
            telegram_token: config.telegram_token,
            telegram_admin_handle: config.telegram_admin_handle,
            api_token: config.api_token,
            tls,
            footer_blurb: config.footer_blurb,
            local_domains,
            local_blurb: config.local_blurb,
            prometheus_config,
            client_pool_size: config.client_pool_size,
        })
    }

    pub(crate) fn prometheus_bind_address(&self) -> Option<SocketAddr> {
        let config = self.prometheus_config.as_ref()?;

        Some((config.addr, config.port).into())
    }

    pub(crate) fn open_keys(&self) -> Result<Option<(Vec<Certificate>, PrivateKey)>, Error> {
        let tls = if let Some(tls) = &self.tls {
            tls
        } else {
            tracing::warn!("No TLS config present");
            return Ok(None);
        };

        let mut certs_reader = BufReader::new(std::fs::File::open(&tls.cert)?);
        let certs = rustls_pemfile::certs(&mut certs_reader)?;

        if certs.is_empty() {
            tracing::warn!("No certs read from certificate file");
            return Ok(None);
        }

        let mut key_reader = BufReader::new(std::fs::File::open(&tls.key)?);
        let key = rustls_pemfile::read_one(&mut key_reader)?;

        let certs = certs.into_iter().map(Certificate).collect();

        let key = if let Some(key) = key {
            match key {
                rustls_pemfile::Item::RSAKey(der) => PrivateKey(der),
                rustls_pemfile::Item::PKCS8Key(der) => PrivateKey(der),
                rustls_pemfile::Item::ECKey(der) => PrivateKey(der),
                _ => {
                    tracing::warn!("Unknown key format: {:?}", key);
                    return Ok(None);
                }
            }
        } else {
            tracing::warn!("Failed to read private key");
            return Ok(None);
        };

        Ok(Some((certs, key)))
    }

    pub(crate) fn footer_blurb(&self) -> Option<crate::templates::Html<String>> {
        if let Some(blurb) = &self.footer_blurb {
            if !blurb.is_empty() {
                return Some(crate::templates::Html(ammonia::clean(blurb)));
            }
        }

        None
    }

    pub(crate) fn local_blurb(&self) -> Option<crate::templates::Html<String>> {
        if let Some(blurb) = &self.local_blurb {
            if !blurb.is_empty() {
                return Some(crate::templates::Html(ammonia::clean(blurb)));
            }
        }

        None
    }

    pub(crate) fn local_domains(&self) -> &[String] {
        &self.local_domains
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

    pub(crate) fn x_api_token(&self) -> Option<XApiToken> {
        self.api_token.clone().map(XApiToken::new)
    }

    pub(crate) fn admin_config(&self) -> Option<actix_web::web::Data<AdminConfig>> {
        if let Some(api_token) = &self.api_token {
            match AdminConfig::build(api_token) {
                Ok(conf) => Some(actix_web::web::Data::new(conf)),
                Err(e) => {
                    tracing::error!("Error creating admin config: {e}");
                    None
                }
            }
        } else {
            None
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
            return format!("v{}-{git}", Self::version());
        }

        format!("v{}", Self::version())
    }

    fn git_version() -> Option<String> {
        let branch = Self::git_branch()?;
        let hash = Self::git_short_hash()?;

        Some(format!("{branch}-{hash}"))
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

    fn git_short_hash() -> Option<&'static str> {
        option_env!("GIT_SHORT_HASH")
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

    pub(crate) fn client_pool_size(&self) -> usize {
        self.client_pool_size
    }

    pub(crate) fn source_code(&self) -> &IriString {
        &self.source_repo
    }

    pub(crate) fn opentelemetry_url(&self) -> Option<&IriString> {
        self.opentelemetry_url.as_ref()
    }

    pub(crate) fn telegram_info(&self) -> Option<(&str, &str)> {
        self.telegram_token.as_deref().and_then(|token| {
            let handle = self.telegram_admin_handle.as_deref()?;
            Some((token, handle))
        })
    }

    pub(crate) fn generate_url(&self, kind: UrlKind) -> IriString {
        self.do_generate_url(kind).expect("Generated valid IRI")
    }

    fn do_generate_url(&self, kind: UrlKind) -> Result<IriString, Error> {
        let iri = match kind {
            UrlKind::Activity => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new(&format!("activity/{}", Uuid::new_v4()))?.as_ref())
                .try_to_dedicated_string()?,
            UrlKind::Actor => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new("actor")?.as_ref())
                .try_to_dedicated_string()?,
            UrlKind::Followers => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new("followers")?.as_ref())
                .try_to_dedicated_string()?,
            UrlKind::Following => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new("following")?.as_ref())
                .try_to_dedicated_string()?,
            UrlKind::Inbox => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new("inbox")?.as_ref())
                .try_to_dedicated_string()?,
            UrlKind::Index => self.base_uri.clone().into(),
            UrlKind::MainKey => {
                let actor = IriRelativeStr::new("actor")?;
                let fragment = IriFragmentStr::new("main-key")?;

                let mut resolved = FixedBaseResolver::new(self.base_uri.as_ref())
                    .resolve(actor.as_ref())
                    .try_to_dedicated_string()?;

                resolved.set_fragment(Some(fragment));
                resolved
            }
            UrlKind::Media(uuid) => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new(&format!("media/{uuid}"))?.as_ref())
                .try_to_dedicated_string()?,
            UrlKind::NodeInfo => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new("nodeinfo/2.0.json")?.as_ref())
                .try_to_dedicated_string()?,
            UrlKind::Outbox => FixedBaseResolver::new(self.base_uri.as_ref())
                .resolve(IriRelativeStr::new("outbox")?.as_ref())
                .try_to_dedicated_string()?,
        };

        Ok(iri)
    }

    pub(crate) fn generate_admin_url(&self, kind: AdminUrlKind) -> IriString {
        self.do_generate_admin_url(kind)
            .expect("Generated valid IRI")
    }

    fn do_generate_admin_url(&self, kind: AdminUrlKind) -> Result<IriString, Error> {
        let path = match kind {
            AdminUrlKind::Allow => "api/v1/admin/allow",
            AdminUrlKind::Disallow => "api/v1/admin/disallow",
            AdminUrlKind::Block => "api/v1/admin/block",
            AdminUrlKind::Unblock => "api/v1/admin/unblock",
            AdminUrlKind::Allowed => "api/v1/admin/allowed",
            AdminUrlKind::Blocked => "api/v1/admin/blocked",
            AdminUrlKind::Connected => "api/v1/admin/connected",
            AdminUrlKind::Stats => "api/v1/admin/stats",
            AdminUrlKind::LastSeen => "api/v1/admin/last_seen",
        };

        let iri = FixedBaseResolver::new(self.base_uri.as_ref())
            .resolve(IriRelativeStr::new(path)?.as_ref())
            .try_to_dedicated_string()?;

        Ok(iri)
    }
}
