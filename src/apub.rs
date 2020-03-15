use activitystreams::{
    object::{Object, ObjectBox},
    primitives::XsdAnyUri,
    PropRefs,
};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PropRefs)]
#[serde(rename_all = "camelCase")]
#[prop_refs(Object)]
pub struct AnyExistingObject {
    pub id: XsdAnyUri,

    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValidTypes {
    Announce,
    Create,
    Delete,
    Follow,
    Undo,
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
}

impl AcceptedActors {
    pub fn inbox(&self) -> &XsdAnyUri {
        self.endpoints.shared_inbox.as_ref().unwrap_or(&self.inbox)
    }
}
