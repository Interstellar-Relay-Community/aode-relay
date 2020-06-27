use crate::{
    config::UrlKind,
    jobs::{cache_media::CacheMedia, JobState},
};
use activitystreams_new::url::Url;
use anyhow::Error;
use background_jobs::ActixJob;
use futures::join;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct QueryInstance {
    listener: Url,
}

impl QueryInstance {
    pub fn new(listener: Url) -> Self {
        QueryInstance {
            listener: listener.into(),
        }
    }

    async fn perform(self, state: JobState) -> Result<(), Error> {
        let (o1, o2) = join!(
            state.node_cache.is_contact_outdated(&self.listener),
            state.node_cache.is_instance_outdated(&self.listener),
        );

        if !(o1 || o2) {
            return Ok(());
        }

        let mut instance_uri = self.listener.clone();
        instance_uri.set_fragment(None);
        instance_uri.set_query(None);
        instance_uri.set_path("api/v1/instance");

        let instance = state
            .requests
            .fetch::<Instance>(instance_uri.as_str())
            .await?;

        let description = if instance.description.is_empty() {
            instance.short_description
        } else {
            instance.description
        };

        if let Some(mut contact) = instance.contact {
            let uuid = if let Some(uuid) = state.media.get_uuid(&contact.avatar).await? {
                contact.avatar = state.config.generate_url(UrlKind::Media(uuid)).into();
                uuid
            } else {
                let uuid = state.media.store_url(&contact.avatar).await?;
                contact.avatar = state.config.generate_url(UrlKind::Media(uuid)).into();
                uuid
            };

            state.job_server.queue(CacheMedia::new(uuid))?;

            state
                .node_cache
                .set_contact(
                    &self.listener,
                    contact.username,
                    contact.display_name,
                    contact.url,
                    contact.avatar,
                )
                .await?;
        }

        let description = ammonia::clean(&description);

        state
            .node_cache
            .set_instance(
                &self.listener,
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
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    const NAME: &'static str = "relay::jobs::QueryInstance";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

#[derive(serde::Deserialize)]
struct Instance {
    title: String,
    short_description: String,
    description: String,
    version: String,
    registrations: bool,
    approval_required: bool,

    #[serde(rename = "contact_account")]
    contact: Option<Contact>,
}

#[derive(serde::Deserialize)]
struct Contact {
    username: String,
    display_name: String,
    url: Url,
    avatar: Url,
}
