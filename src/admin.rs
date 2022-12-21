use activitystreams::iri_string::types::IriString;
use std::collections::{BTreeMap, BTreeSet};
use time::OffsetDateTime;

pub mod client;
pub mod routes;

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct Domains {
    domains: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct AllowedDomains {
    pub(crate) allowed_domains: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct BlockedDomains {
    pub(crate) blocked_domains: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ConnectedActors {
    pub(crate) connected_actors: Vec<IriString>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct LastSeen {
    pub(crate) last_seen: BTreeMap<OffsetDateTime, BTreeSet<String>>,
    pub(crate) never: Vec<String>,
}
