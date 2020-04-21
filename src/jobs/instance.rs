use crate::{config::UrlKind, jobs::JobState};
use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use background_jobs::ActixJob;
use futures::join;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct QueryInstance {
    listener: XsdAnyUri,
}

impl QueryInstance {
    pub fn new(listener: XsdAnyUri) -> Self {
        QueryInstance { listener }
    }

    async fn perform(mut self, state: JobState) -> Result<(), Error> {
        let listener = self.listener.clone();

        let (o1, o2) = join!(
            state.node_cache.is_contact_outdated(&listener),
            state.node_cache.is_instance_outdated(&listener),
        );

        if !(o1 || o2) {
            return Ok(());
        }

        let url = self.listener.as_url_mut();
        url.set_fragment(None);
        url.set_query(None);
        url.set_path("api/v1/instance");

        let instance = state
            .requests
            .fetch::<Instance>(self.listener.as_str())
            .await?;

        let description = if instance.description.is_empty() {
            instance.short_description
        } else {
            instance.description
        };

        if let Some(mut contact) = instance.contact {
            if let Some(uuid) = state.media.get_uuid(&contact.avatar).await? {
                contact.avatar = state.config.generate_url(UrlKind::Media(uuid)).parse()?;
            } else {
                let uuid = state.media.store_url(&contact.avatar).await?;
                contact.avatar = state.config.generate_url(UrlKind::Media(uuid)).parse()?;
            }

            state
                .node_cache
                .set_contact(
                    &listener,
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
                &listener,
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

    const NAME: &'static str = "InstanceProcessor";

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
    url: XsdAnyUri,
    avatar: XsdAnyUri,
}
