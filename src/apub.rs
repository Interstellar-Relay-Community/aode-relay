use activitystreams::{
    object::{Object, ObjectBox},
    primitives::XsdAnyUri,
    PropRefs,
};
use std::collections::HashMap;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PropRefs)]
#[serde(rename_all = "camelCase")]
#[prop_refs(Object)]
pub struct AnyExistingObject {
    pub id: XsdAnyUri,

    #[serde(rename = "type")]
    pub kind: String,

    #[serde(flatten)]
    ext: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValidTypes {
    Announce,
    Create,
    Delete,
    Follow,
    Undo,
    Update,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
#[serde(rename_all = "camelCase")]
pub enum ValidObjects {
    Id(XsdAnyUri),
    Object(AnyExistingObject),
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedObjects {
    pub id: XsdAnyUri,

    #[serde(rename = "type")]
    pub kind: ValidTypes,

    pub actor: XsdAnyUri,

    pub object: ValidObjects,

    #[serde(flatten)]
    ext: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedActors {
    pub id: XsdAnyUri,

    #[serde(rename = "type")]
    pub kind: String,

    pub inbox: XsdAnyUri,

    pub endpoints: Endpoints,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoints {
    shared_inbox: Option<XsdAnyUri>,
}

impl ValidObjects {
    pub fn id(&self) -> &XsdAnyUri {
        match self {
            ValidObjects::Id(ref id) => id,
            ValidObjects::Object(ref obj) => &obj.id,
        }
    }

    pub fn is_kind(&self, query_kind: &str) -> bool {
        match self {
            ValidObjects::Id(_) => false,
            ValidObjects::Object(AnyExistingObject { kind, .. }) => kind == query_kind,
        }
    }

    pub fn child_object_is_actor(&self) -> bool {
        match self {
            ValidObjects::Id(_) => false,
            ValidObjects::Object(AnyExistingObject { ext, .. }) => {
                if let Some(o) = ext.get("object") {
                    if let Ok(s) = serde_json::from_value::<String>(o.clone()) {
                        return s.ends_with("/actor");
                    }
                }

                false
            }
        }
    }
}

impl AcceptedActors {
    pub fn inbox(&self) -> &XsdAnyUri {
        self.endpoints.shared_inbox.as_ref().unwrap_or(&self.inbox)
    }
}
