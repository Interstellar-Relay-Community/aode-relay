use activitystreams_ext::{Ext1, UnparsedExtension};
use activitystreams_new::{
    activity::ActorAndObject,
    actor::{Actor, ApActor},
    primitives::XsdAnyUri,
    unparsed::UnparsedMutExt,
};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeyInner {
    pub id: XsdAnyUri,
    pub owner: XsdAnyUri,
    pub public_key_pem: String,
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
    Announce,
    Create,
    Delete,
    Follow,
    Reject,
    Undo,
    Update,
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
