use crate::{
    apub::AcceptedActors,
    error::{Error, ErrorKind},
    jobs::JobState,
};
use activitystreams::{iri_string::types::IriString, object::Image, prelude::*};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct QueryContact {
    actor_id: IriString,
    contact_id: IriString,
}

impl std::fmt::Debug for QueryContact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryContact")
            .field("actor_id", &self.actor_id.to_string())
            .field("contact_id", &self.contact_id.to_string())
            .finish()
    }
}

impl QueryContact {
    pub(crate) fn new(actor_id: IriString, contact_id: IriString) -> Self {
        QueryContact {
            actor_id,
            contact_id,
        }
    }

    async fn perform(self, state: JobState) -> Result<(), Error> {
        let contact_outdated = state
            .node_cache
            .is_contact_outdated(self.actor_id.clone())
            .await;

        if !contact_outdated {
            return Ok(());
        }

        let contact = match state
            .requests
            .fetch::<AcceptedActors>(&self.contact_id)
            .await
        {
            Ok(contact) => contact,
            Err(e) if e.is_breaker() => {
                tracing::debug!("Not retrying due to failed breaker");
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        let (username, display_name, url, avatar) =
            to_contact(contact).ok_or(ErrorKind::Extract("contact"))?;

        state
            .node_cache
            .set_contact(self.actor_id, username, display_name, url, avatar)
            .await?;

        Ok(())
    }
}

fn to_contact(contact: AcceptedActors) -> Option<(String, String, IriString, IriString)> {
    let username = contact.preferred_username()?.to_owned();
    let display_name = contact.name()?.as_one()?.as_xsd_string()?.to_owned();

    let url = contact.url()?.as_single_id()?.to_owned();
    let any_base = contact.icon()?.as_one()?;

    let avatar = Image::from_any_base(any_base.clone())
        .ok()??
        .url()?
        .as_single_id()?
        .to_owned();

    Some((username, display_name, url, avatar))
}

impl ActixJob for QueryContact {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::QueryContact";
    const QUEUE: &'static str = "maintenance";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}

#[cfg(test)]
mod tests {
    use super::to_contact;

    const HYNET_ADMIN: &str = r#"{"@context":["https://www.w3.org/ns/activitystreams","https://soc.hyena.network/schemas/litepub-0.1.jsonld",{"@language":"und"}],"alsoKnownAs":[],"attachment":[{"name":"Website","type":"PropertyValue","value":"https://hyena.network/"},{"name":"Services","type":"PropertyValue","value":"Pleroma, Invidious, SearX, XMPP"},{"name":"CW","type":"PropertyValue","value":"all long posts"}],"capabilities":{"acceptsChatMessages":true},"discoverable":true,"endpoints":{"oauthAuthorizationEndpoint":"https://soc.hyena.network/oauth/authorize","oauthRegistrationEndpoint":"https://soc.hyena.network/api/v1/apps","oauthTokenEndpoint":"https://soc.hyena.network/oauth/token","sharedInbox":"https://soc.hyena.network/inbox","uploadMedia":"https://soc.hyena.network/api/ap/upload_media"},"followers":"https://soc.hyena.network/users/HyNET/followers","following":"https://soc.hyena.network/users/HyNET/following","icon":{"type":"Image","url":"https://soc.hyena.network/media/ab149b1e0196ffdbecc6830c7f6f1a14dd8d8408ec7db0f1e8ad9d40e600ea73.gif"},"id":"https://soc.hyena.network/users/HyNET","image":{"type":"Image","url":"https://soc.hyena.network/media/12ba78d3015e13aa65ac4e106e574dd7bf959614585f10ce85de40e0148da677.png"},"inbox":"https://soc.hyena.network/users/HyNET/inbox","manuallyApprovesFollowers":false,"name":"HyNET Announcement System :glider:","outbox":"https://soc.hyena.network/users/HyNET/outbox","preferredUsername":"HyNET","publicKey":{"id":"https://soc.hyena.network/users/HyNET#main-key","owner":"https://soc.hyena.network/users/HyNET","publicKeyPem":"-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAyF74womumWRhR7RW4Q6a\n2+Av/Ue8QHiKwjQARJEakbKnKgkI5FRFVVOfMiYVJp/juNt4GLgK15panBqJa9Yt\nWACiHQjBd2yVI5tIHiae0uBj5SdUVuduoycVLG0lpJsg12p8m/vL1oaeLqehTqa6\nsYplQh1GCLet0cUdn/66Cj2pAPD3V7Bz3VnG+oyXIsGQbBB8RHnWhFH8b0qQOyur\nJRAB8aye6QAL2sQbfISM2lycWzNeIHkqsUb7FdqdhQ+Ze0rETRGDkOO2Qvpg0hQm\n6owMsHnHA/DzyOHLy6Yf+I3OUlBC/P1SSAKwORsifFDXL322AEqoDi5ZpwzG9m5z\nAQIDAQAB\n-----END PUBLIC KEY-----\n\n"},"summary":"Ran by <span class=\"h-card\"><a class=\"u-url mention\" data-user=\"9s8j4AHGt3ED0P0b6e\" href=\"https://soc.hyena.network/users/mel\" rel=\"ugc\">@<span>mel</span></a></span> :adm1::adm2: <br/>For direct help with the service, send <span class=\"h-card\"><a class=\"u-url mention\" data-user=\"9s8j4AHGt3ED0P0b6e\" href=\"https://soc.hyena.network/users/mel\" rel=\"ugc\">@<span>mel</span></a></span> a message.","tag":[{"icon":{"type":"Image","url":"https://soc.hyena.network/emoji/Signs/adm1.png"},"id":"https://soc.hyena.network/emoji/Signs/adm1.png","name":":adm1:","type":"Emoji","updated":"1970-01-01T00:00:00Z"},{"icon":{"type":"Image","url":"https://soc.hyena.network/emoji/Signs/adm2.png"},"id":"https://soc.hyena.network/emoji/Signs/adm2.png","name":":adm2:","type":"Emoji","updated":"1970-01-01T00:00:00Z"},{"icon":{"type":"Image","url":"https://soc.hyena.network/emoji/misc/glider.png"},"id":"https://soc.hyena.network/emoji/misc/glider.png","name":":glider:","type":"Emoji","updated":"1970-01-01T00:00:00Z"}],"type":"Service","url":"https://soc.hyena.network/users/HyNET"}"#;

    #[test]
    fn parse_hynet() {
        let actor = serde_json::from_str(HYNET_ADMIN).unwrap();
        to_contact(actor).unwrap();
    }
}
