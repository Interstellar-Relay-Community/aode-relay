use activitystreams::{
    activity::ActorAndObject,
    actor::{Actor, ApActor},
    iri_string::types::IriString,
    unparsed::UnparsedMutExt,
};
use activitystreams_ext::{Ext1, UnparsedExtension};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeyInner {
    pub id: IriString,
    pub owner: IriString,
    pub public_key_pem: String,
}

impl std::fmt::Debug for PublicKeyInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicKeyInner")
            .field("id", &self.id.to_string())
            .field("owner", &self.owner.to_string())
            .field("public_key_pem", &self.public_key_pem)
            .finish()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKey {
    pub public_key: PublicKeyInner,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ValidTypes {
    Accept,
    Add,
    Announce,
    Create,
    Delete,
    Follow,
    Reject,
    Remove,
    Undo,
    Update,
    Move,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum UndoTypes {
    Follow,
    Announce,
    Create,
}

pub type AcceptedUndoObjects = ActorAndObject<UndoTypes>;
pub type AcceptedActivities = ActorAndObject<ValidTypes>;
pub type AcceptedActors = Ext1<ApActor<Actor<String>>, PublicKey>;

impl<U> UnparsedExtension<U> for PublicKey
where
    U: UnparsedMutExt,
{
    type Error = serde_json::Error;

    fn try_from_unparsed(unparsed_mut: &mut U) -> Result<Self, Self::Error> {
        Ok(PublicKey {
            public_key: unparsed_mut.remove("publicKey")?,
        })
    }

    fn try_into_unparsed(self, unparsed_mut: &mut U) -> Result<(), Self::Error> {
        unparsed_mut.insert("publicKey", self.public_key)?;
        Ok(())
    }
}
