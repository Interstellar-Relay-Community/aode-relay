use activitystreams::{
    actor::Actor,
    ext::Extension,
    object::{Object, ObjectBox},
    primitives::XsdAnyUri,
    Base, BaseBox, PropRefs,
};
use std::collections::HashMap;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKey {
    pub id: XsdAnyUri,
    pub owner: XsdAnyUri,
    pub public_key_pem: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeyExtension {
    pub public_key: PublicKey,
}

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize, PropRefs)]
#[serde(rename_all = "camelCase")]
#[prop_refs(Object)]
pub struct AnyExistingObject {
    pub id: XsdAnyUri,

    #[serde(rename = "type")]
    pub kind: String,

    #[serde(flatten)]
    ext: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValidTypes {
    Accept,
    Announce,
    Create,
    Delete,
    Follow,
    Reject,
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

    pub public_key: PublicKey,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoints {
    shared_inbox: Option<XsdAnyUri>,
}

impl PublicKey {
    pub fn to_ext(self) -> PublicKeyExtension {
        self.into()
    }
}

impl From<PublicKey> for PublicKeyExtension {
    fn from(public_key: PublicKey) -> Self {
        PublicKeyExtension { public_key }
    }
}

impl<T> Extension<T> for PublicKeyExtension where T: Actor {}

impl ValidObjects {
    pub fn id(&self) -> &XsdAnyUri {
        match self {
            ValidObjects::Id(ref id) => id,
            ValidObjects::Object(ref obj) => &obj.id,
        }
    }

    pub fn kind(&self) -> Option<&str> {
        match self {
            ValidObjects::Id(_) => None,
            ValidObjects::Object(AnyExistingObject { kind, .. }) => Some(kind),
        }
    }

    pub fn is_kind(&self, query_kind: &str) -> bool {
        match self {
            ValidObjects::Id(_) => false,
            ValidObjects::Object(AnyExistingObject { kind, .. }) => kind == query_kind,
        }
    }

    pub fn is(&self, uri: &XsdAnyUri) -> bool {
        match self {
            ValidObjects::Id(id) => id == uri,
            ValidObjects::Object(AnyExistingObject { id, .. }) => id == uri,
        }
    }

    pub fn child_object_id(&self) -> Option<XsdAnyUri> {
        match self {
            ValidObjects::Id(_) => None,
            ValidObjects::Object(AnyExistingObject { ext, .. }) => {
                if let Some(o) = ext.get("object") {
                    if let Ok(child_uri) = serde_json::from_value::<XsdAnyUri>(o.clone()) {
                        return Some(child_uri);
                    }
                }

                None
            }
        }
    }

    pub fn child_object_is(&self, uri: &XsdAnyUri) -> bool {
        if let Some(child_object_id) = self.child_object_id() {
            return *uri == child_object_id;
        }
        false
    }

    pub fn child_actor_id(&self) -> Option<XsdAnyUri> {
        match self {
            ValidObjects::Id(_) => None,
            ValidObjects::Object(AnyExistingObject { ext, .. }) => {
                if let Some(o) = ext.get("actor") {
                    if let Ok(child_uri) = serde_json::from_value::<XsdAnyUri>(o.clone()) {
                        return Some(child_uri);
                    }
                }

                None
            }
        }
    }

    pub fn child_actor_is(&self, uri: &XsdAnyUri) -> bool {
        if let Some(child_actor_id) = self.child_actor_id() {
            return *uri == child_actor_id;
        }
        false
    }
}

impl AcceptedActors {
    pub fn inbox(&self) -> &XsdAnyUri {
        self.endpoints.shared_inbox.as_ref().unwrap_or(&self.inbox)
    }
}
