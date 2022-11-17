use activitystreams::iri_string::types::IriString;

pub mod client;
pub mod routes;

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct Domains {
    domains: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct AllowedDomains {
    allowed_domains: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct BlockedDomains {
    blocked_domains: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct ConnectedActors {
    connected_actors: Vec<IriString>,
}
