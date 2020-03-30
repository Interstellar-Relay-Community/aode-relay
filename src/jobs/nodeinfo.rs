use crate::jobs::JobState;
use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use background_jobs::{ActixJob, Processor};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct QueryNodeinfo {
    listener: XsdAnyUri,
}

impl QueryNodeinfo {
    pub fn new(listener: XsdAnyUri) -> Self {
        QueryNodeinfo { listener }
    }

    async fn perform(mut self, state: JobState) -> Result<(), Error> {
        let listener = self.listener.clone();

        if !state.node_cache.is_nodeinfo_outdated(&listener).await {
            return Ok(());
        }

        let url = self.listener.as_url_mut();
        url.set_fragment(None);
        url.set_query(None);
        url.set_path(".well-known/nodeinfo");

        let well_known = state
            .requests
            .fetch::<WellKnown>(self.listener.as_str())
            .await?;

        let href = if let Some(link) = well_known.links.into_iter().next() {
            link.href
        } else {
            return Ok(());
        };

        let nodeinfo = state.requests.fetch::<Nodeinfo>(&href).await?;

        state
            .node_cache
            .set_info(
                &listener,
                nodeinfo.software.name,
                nodeinfo.software.version,
                nodeinfo.open_registrations,
            )
            .await?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct NodeinfoProcessor;

impl ActixJob for QueryNodeinfo {
    type State = JobState;
    type Processor = NodeinfoProcessor;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

impl Processor for NodeinfoProcessor {
    type Job = QueryNodeinfo;

    const NAME: &'static str = "NodeinfoProcessor";
    const QUEUE: &'static str = "default";
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Nodeinfo {
    #[allow(dead_code)]
    version: SupportedVersion,

    software: Software,
    open_registrations: bool,
}

#[derive(serde::Deserialize)]
struct Software {
    name: String,
    version: String,
}

#[derive(serde::Deserialize)]
struct WellKnown {
    links: Vec<Link>,
}

#[derive(serde::Deserialize)]
struct Link {
    #[allow(dead_code)]
    rel: SupportedNodeinfo,

    href: String,
}

struct SupportedVersion;
struct SupportedNodeinfo;

static SUPPORTED_VERSION: &'static str = "2.0";
static SUPPORTED_NODEINFO: &'static str = "http://nodeinfo.diaspora.software/ns/schema/2.0";

struct SupportedVersionVisitor;
struct SupportedNodeinfoVisitor;

impl<'de> serde::de::Visitor<'de> for SupportedVersionVisitor {
    type Value = SupportedVersion;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "the string '{}'", SUPPORTED_VERSION)
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if s == SUPPORTED_VERSION {
            Ok(SupportedVersion)
        } else {
            Err(serde::de::Error::custom("Invalid nodeinfo version"))
        }
    }
}

impl<'de> serde::de::Visitor<'de> for SupportedNodeinfoVisitor {
    type Value = SupportedNodeinfo;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "the string '{}'", SUPPORTED_NODEINFO)
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if s == SUPPORTED_NODEINFO {
            Ok(SupportedNodeinfo)
        } else {
            Err(serde::de::Error::custom("Invalid nodeinfo version"))
        }
    }
}

impl<'de> serde::de::Deserialize<'de> for SupportedVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(SupportedVersionVisitor)
    }
}

impl<'de> serde::de::Deserialize<'de> for SupportedNodeinfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(SupportedNodeinfoVisitor)
    }
}
