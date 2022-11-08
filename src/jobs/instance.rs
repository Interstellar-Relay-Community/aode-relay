use crate::{
    config::UrlKind,
    error::{Error, ErrorKind},
    jobs::{cache_media::CacheMedia, JobState},
};
use activitystreams::{iri, iri_string::types::IriString};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct QueryInstance {
    actor_id: IriString,
}

impl std::fmt::Debug for QueryInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryInstance")
            .field("actor_id", &self.actor_id.to_string())
            .finish()
    }
}

impl QueryInstance {
    pub(crate) fn new(actor_id: IriString) -> Self {
        QueryInstance { actor_id }
    }

    #[tracing::instrument(name = "Query instance", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let contact_outdated = state
            .node_cache
            .is_contact_outdated(self.actor_id.clone())
            .await;
        let instance_outdated = state
            .node_cache
            .is_instance_outdated(self.actor_id.clone())
            .await;

        if !(contact_outdated || instance_outdated) {
            return Ok(());
        }

        let authority = self
            .actor_id
            .authority_str()
            .ok_or(ErrorKind::MissingDomain)?;
        let scheme = self.actor_id.scheme_str();
        let instance_uri = iri!(format!("{}://{}/api/v1/instance", scheme, authority));

        let instance = state
            .requests
            .fetch_json::<Instance>(instance_uri.as_str())
            .await?;

        let description = instance.short_description.unwrap_or(instance.description);

        if let Some(contact) = instance.contact {
            let uuid = if let Some(uuid) = state.media.get_uuid(contact.avatar.clone()).await? {
                uuid
            } else {
                state.media.store_url(contact.avatar).await?
            };

            let avatar = state.config.generate_url(UrlKind::Media(uuid));

            state.job_server.queue(CacheMedia::new(uuid)).await?;

            state
                .node_cache
                .set_contact(
                    self.actor_id.clone(),
                    contact.username,
                    contact.display_name,
                    contact.url,
                    avatar,
                )
                .await?;
        }

        let description = ammonia::clean(&description);

        state
            .node_cache
            .set_instance(
                self.actor_id,
                instance.title,
                description,
                instance.version,
                instance.registrations,
                instance.approval_required,
            )
            .await?;

        Ok(())
    }
}

impl ActixJob for QueryInstance {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::QueryInstance";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}

fn default_approval() -> bool {
    false
}

#[derive(serde::Deserialize)]
struct Instance {
    title: String,
    short_description: Option<String>,
    description: String,
    version: String,
    registrations: bool,

    #[serde(default = "default_approval")]
    approval_required: bool,

    #[serde(rename = "contact_account")]
    contact: Option<Contact>,
}

#[derive(serde::Deserialize)]
struct Contact {
    username: String,
    display_name: String,
    url: IriString,
    avatar: IriString,
}

#[cfg(test)]
mod tests {
    use super::Instance;

    const ASONIX_INSTANCE: &'static str = r#"{"uri":"masto.asonix.dog","title":"asonix.dog","short_description":"The asonix of furry mastodon. For me and a few friends. DM me somewhere if u want an account lol","description":"A mastodon server that's only for me and nobody else sorry","email":"asonix@asonix.dog","version":"4.0.0rc2-asonix-changes","urls":{"streaming_api":"wss://masto.asonix.dog"},"stats":{"user_count":7,"status_count":12328,"domain_count":5146},"thumbnail":"https://masto.asonix.dog/system/site_uploads/files/000/000/002/@1x/32f51462a2b2bf2d.png","languages":["dog"],"registrations":false,"approval_required":false,"invites_enabled":false,"configuration":{"accounts":{"max_featured_tags":10},"statuses":{"max_characters":500,"max_media_attachments":4,"characters_reserved_per_url":23},"media_attachments":{"supported_mime_types":["image/jpeg","image/png","image/gif","image/heic","image/heif","image/webp","image/avif","video/webm","video/mp4","video/quicktime","video/ogg","audio/wave","audio/wav","audio/x-wav","audio/x-pn-wave","audio/vnd.wave","audio/ogg","audio/vorbis","audio/mpeg","audio/mp3","audio/webm","audio/flac","audio/aac","audio/m4a","audio/x-m4a","audio/mp4","audio/3gpp","video/x-ms-asf"],"image_size_limit":10485760,"image_matrix_limit":16777216,"video_size_limit":41943040,"video_frame_rate_limit":60,"video_matrix_limit":2304000},"polls":{"max_options":4,"max_characters_per_option":50,"min_expiration":300,"max_expiration":2629746}},"contact_account":{"id":"1","username":"asonix","acct":"asonix","display_name":"Liom on Mane :antiverified:","locked":true,"bot":false,"discoverable":true,"group":false,"created_at":"2021-02-09T00:00:00.000Z","note":"\u003cp\u003e26, local liom, friend, rust (lang) stan, bi \u003c/p\u003e\u003cp\u003eicon by \u003cspan class=\"h-card\"\u003e\u003ca href=\"https://furaffinity.net/user/lalupine\" target=\"blank\" rel=\"noopener noreferrer\" class=\"u-url mention\"\u003e@\u003cspan\u003elalupine@furaffinity.net\u003c/span\u003e\u003c/a\u003e\u003c/span\u003e\u003cbr /\u003eheader by \u003cspan class=\"h-card\"\u003e\u003ca href=\"https://furaffinity.net/user/tronixx\" target=\"blank\" rel=\"noopener noreferrer\" class=\"u-url mention\"\u003e@\u003cspan\u003etronixx@furaffinity.net\u003c/span\u003e\u003c/a\u003e\u003c/span\u003e\u003c/p\u003e\u003cp\u003eTestimonials:\u003c/p\u003e\u003cp\u003eStand: LIONS\u003cbr /\u003eStand User: AODE\u003cbr /\u003e- Keris (not on here)\u003c/p\u003e","url":"https://masto.asonix.dog/@asonix","avatar":"https://masto.asonix.dog/system/accounts/avatars/000/000/001/original/00852df0e6fee7e0.png","avatar_static":"https://masto.asonix.dog/system/accounts/avatars/000/000/001/original/00852df0e6fee7e0.png","header":"https://masto.asonix.dog/system/accounts/headers/000/000/001/original/8122ce3e5a745385.png","header_static":"https://masto.asonix.dog/system/accounts/headers/000/000/001/original/8122ce3e5a745385.png","followers_count":237,"following_count":474,"statuses_count":8798,"last_status_at":"2022-11-08","noindex":true,"emojis":[{"shortcode":"antiverified","url":"https://masto.asonix.dog/system/custom_emojis/images/000/030/053/original/bb0bc2e395b9a127.png","static_url":"https://masto.asonix.dog/system/custom_emojis/images/000/030/053/static/bb0bc2e395b9a127.png","visible_in_picker":true}],"fields":[{"name":"pronouns","value":"he/they","verified_at":null},{"name":"software","value":"bad","verified_at":null},{"name":"gitea","value":"\u003ca href=\"https://git.asonix.dog\" target=\"_blank\" rel=\"nofollow noopener noreferrer me\"\u003e\u003cspan class=\"invisible\"\u003ehttps://\u003c/span\u003e\u003cspan class=\"\"\u003egit.asonix.dog\u003c/span\u003e\u003cspan class=\"invisible\"\u003e\u003c/span\u003e\u003c/a\u003e","verified_at":null},{"name":"join my","value":"relay","verified_at":null}]},"rules":[]}"#;

    #[test]
    fn deser_masto_instance_with_contact() {
        let inst: Instance = serde_json::from_str(ASONIX_INSTANCE).unwrap();
        let _ = inst.contact.unwrap();
    }
}
