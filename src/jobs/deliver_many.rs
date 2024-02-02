use crate::{
    error::Error,
    future::BoxFuture,
    jobs::{debug_object, Deliver, JobState},
};
use activitystreams::iri_string::types::IriString;
use background_jobs::Job;
use rand::Rng;

use crate::data::NodeConfig;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct DeliverMany {
    to: Vec<IriString>,
    filterable: bool,
    data: serde_json::Value,
    actor_authority: String,
}

impl std::fmt::Debug for DeliverMany {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliverMany")
            .field("activity", &self.data["type"])
            .field("object", debug_object(&self.data))
            .finish()
    }
}

impl DeliverMany {
    pub(crate) fn new<T>(to: Vec<IriString>, data: T, actor_authority: String, filterable: bool) -> Result<Self, Error>
    where
        T: serde::ser::Serialize,
    {
        Ok(DeliverMany {
            to,
            filterable,
            data: serde_json::to_value(data)?,
            actor_authority,
        })
    }

    fn apply_filter(dice: u8, authority: &str, config: &NodeConfig) -> bool {
        if config.enable_probability && config.probability < dice {
            return false;
        }

        let has_authority = config.authority_set.contains(authority);

        if config.is_allowlist {
            has_authority
        } else {
            !has_authority
        }
    }

    #[tracing::instrument(name = "Deliver many", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let dice = rand::thread_rng().gen::<u8>();

        for inbox in self.to {
            if self.filterable {
                // All inbox should have... authority... but...
                let inbox_authority = inbox.authority_str().unwrap_or("");

                let node_config = match state.state.node_config.read() {
                    Ok(node_config) => node_config,
                    Err(e) => {
                        tracing::error!("Failed to acquire read lock for node config: {}", e);
                        continue;
                    }
                };

                if let Some(cfg) = node_config.get(inbox_authority) {
                    if !Self::apply_filter(dice, &self.actor_authority, cfg) {
                        tracing::info!("Skipping egress to {} due to given criteria", inbox_authority);
                        continue;
                    }
                }
            }

            state
                .job_server
                .queue(Deliver::new(inbox, self.data.clone())?)
                .await?;
        }

        Ok(())
    }
}

impl Job for DeliverMany {
    type State = JobState;
    type Future = BoxFuture<'static, anyhow::Result<()>>;

    const NAME: &'static str = "relay::jobs::DeliverMany";
    const QUEUE: &'static str = "deliver";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
