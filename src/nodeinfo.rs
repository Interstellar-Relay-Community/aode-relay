use crate::state::{State, UrlKind};
use actix_web::{web, Responder};
use actix_webfinger::Link;
use std::collections::HashMap;

pub async fn well_known(state: web::Data<State>) -> impl Responder {
    web::Json(Link {
        rel: "http://nodeinfo.diaspora.software/ns/schema/2.0".to_owned(),
        href: Some(state.generate_url(UrlKind::NodeInfo)),
        template: None,
        kind: None,
    })
    .with_header("Content-Type", "application/jrd+json")
}

pub async fn route(state: web::Data<State>) -> web::Json<NodeInfo> {
    web::Json(NodeInfo {
        version: NodeInfoVersion,
        software: Software {
            name: state.software_name(),
            version: state.software_version(),
        },
        protocols: vec![Protocol::ActivityPub],
        services: vec![],
        open_registrations: false,
        usage: Usage {
            local_posts: 0,
            local_comments: 0,
        },
        metadata: Metadata::default(),
    })
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    version: NodeInfoVersion,
    software: Software,
    protocols: Vec<Protocol>,
    services: Vec<Service>,
    open_registrations: bool,
    usage: Usage,
    metadata: Metadata,
}

#[derive(Clone, Debug, Default)]
pub struct NodeInfoVersion;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Software {
    name: String,
    version: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub enum Protocol {
    ActivityPub,
}

#[derive(Clone, Debug, serde::Serialize)]
pub enum Service {}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    local_posts: u64,
    local_comments: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(transparent)]
pub struct Metadata(pub HashMap<String, serde_json::Value>);

impl serde::ser::Serialize for NodeInfoVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str("2.0")
    }
}
