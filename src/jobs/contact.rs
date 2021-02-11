use crate::{apub::AcceptedActors, jobs::JobState};
use activitystreams::{object::Image, prelude::*, url::Url};
use anyhow::Error;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct QueryContact {
    actor_id: Url,
    contact_id: Url,
}

impl QueryContact {
    pub(crate) fn new(actor_id: Url, contact_id: Url) -> Self {
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

        let contact = state
            .requests
            .fetch::<AcceptedActors>(self.contact_id.as_str())
            .await?;

        let username = contact.preferred_username();
        let display_name = contact
            .name()
            .and_then(|name| name.as_one().and_then(|s| s.as_xsd_string()));
        let url = contact.url().and_then(|url| url.as_single_id());
        let avatar = contact
            .icon()
            .and_then(|one_or_many| one_or_many.as_one())
            .and_then(|any_base| Image::from_any_base(any_base.clone()).ok()?)
            .and_then(|image| {
                image
                    .url()
                    .and_then(|url| url.as_single_id())
                    .map(|url| url.to_owned())
            });

        let optioned = || {
            let username = username?;
            let display_name = display_name?;
            let url = url?;
            let avatar = avatar?;

            Some(state.node_cache.set_contact(
                self.actor_id.clone(),
                username.to_owned(),
                display_name.to_owned(),
                url.to_owned(),
                avatar.to_owned(),
            ))
        };

        if let Some(fut) = (optioned)() {
            fut.await?;
        }

        Ok(())
    }
}

impl ActixJob for QueryContact {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    const NAME: &'static str = "relay::jobs::QueryContact";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

#[cfg(test)]
mod tests {
    use crate::apub::AcceptedActors;

    const HYNET_ADMIN: &'static str = r#"{"@context":["https://www.w3.org/ns/activitystreams","https://soc.hyena.network/schemas/litepub-0.1.jsonld",{"@language":"und"}],"alsoKnownAs":[],"attachment":[{"name":"Website","type":"PropertyValue","value":"https://hyena.network/"},{"name":"Services","type":"PropertyValue","value":"Pleroma, Invidious, SearX, XMPP"},{"name":"CW","type":"PropertyValue","value":"all long posts"}],"capabilities":{"acceptsChatMessages":true},"discoverable":true,"endpoints":{"oauthAuthorizationEndpoint":"https://soc.hyena.network/oauth/authorize","oauthRegistrationEndpoint":"https://soc.hyena.network/api/v1/apps","oauthTokenEndpoint":"https://soc.hyena.network/oauth/token","sharedInbox":"https://soc.hyena.network/inbox","uploadMedia":"https://soc.hyena.network/api/ap/upload_media"},"followers":"https://soc.hyena.network/users/HyNET/followers","following":"https://soc.hyena.network/users/HyNET/following","icon":{"type":"Image","url":"https://soc.hyena.network/media/ab149b1e0196ffdbecc6830c7f6f1a14dd8d8408ec7db0f1e8ad9d40e600ea73.gif"},"id":"https://soc.hyena.network/users/HyNET","image":{"type":"Image","url":"https://soc.hyena.network/media/12ba78d3015e13aa65ac4e106e574dd7bf959614585f10ce85de40e0148da677.png"},"inbox":"https://soc.hyena.network/users/HyNET/inbox","manuallyApprovesFollowers":false,"name":"HyNET Announcement System :glider:","outbox":"https://soc.hyena.network/users/HyNET/outbox","preferredUsername":"HyNET","publicKey":{"id":"https://soc.hyena.network/users/HyNET#main-key","owner":"https://soc.hyena.network/users/HyNET","publicKeyPem":"-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAyF74womumWRhR7RW4Q6a\n2+Av/Ue8QHiKwjQARJEakbKnKgkI5FRFVVOfMiYVJp/juNt4GLgK15panBqJa9Yt\nWACiHQjBd2yVI5tIHiae0uBj5SdUVuduoycVLG0lpJsg12p8m/vL1oaeLqehTqa6\nsYplQh1GCLet0cUdn/66Cj2pAPD3V7Bz3VnG+oyXIsGQbBB8RHnWhFH8b0qQOyur\nJRAB8aye6QAL2sQbfISM2lycWzNeIHkqsUb7FdqdhQ+Ze0rETRGDkOO2Qvpg0hQm\n6owMsHnHA/DzyOHLy6Yf+I3OUlBC/P1SSAKwORsifFDXL322AEqoDi5ZpwzG9m5z\nAQIDAQAB\n-----END PUBLIC KEY-----\n\n"},"summary":"Ran by <span class=\"h-card\"><a class=\"u-url mention\" data-user=\"9s8j4AHGt3ED0P0b6e\" href=\"https://soc.hyena.network/users/mel\" rel=\"ugc\">@<span>mel</span></a></span> :adm1::adm2: <br/>For direct help with the service, send <span class=\"h-card\"><a class=\"u-url mention\" data-user=\"9s8j4AHGt3ED0P0b6e\" href=\"https://soc.hyena.network/users/mel\" rel=\"ugc\">@<span>mel</span></a></span> a message.","tag":[{"icon":{"type":"Image","url":"https://soc.hyena.network/emoji/Signs/adm1.png"},"id":"https://soc.hyena.network/emoji/Signs/adm1.png","name":":adm1:","type":"Emoji","updated":"1970-01-01T00:00:00Z"},{"icon":{"type":"Image","url":"https://soc.hyena.network/emoji/Signs/adm2.png"},"id":"https://soc.hyena.network/emoji/Signs/adm2.png","name":":adm2:","type":"Emoji","updated":"1970-01-01T00:00:00Z"},{"icon":{"type":"Image","url":"https://soc.hyena.network/emoji/misc/glider.png"},"id":"https://soc.hyena.network/emoji/misc/glider.png","name":":glider:","type":"Emoji","updated":"1970-01-01T00:00:00Z"}],"type":"Service","url":"https://soc.hyena.network/users/HyNET"}"#;

    #[test]
    fn parse_hynet() {
        serde_json::from_str::<AcceptedActors>(HYNET_ADMIN).unwrap();
    }
}
