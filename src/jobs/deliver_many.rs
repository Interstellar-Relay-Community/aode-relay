use crate::{
    error::Error,
    jobs::{debug_object, Deliver, JobState},
};
use activitystreams::iri_string::types::IriString;
use background_jobs::ActixJob;
use futures_util::future::LocalBoxFuture;
use rand::{rngs::ThreadRng, Rng};

use crate::data::NodeConfig;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct DeliverMany {
    to: Vec<IriString>,
    filterable: bool,
    data: serde_json::Value,
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
    pub(crate) fn new<T>(to: Vec<IriString>, data: T, filterable: bool) -> Result<Self, Error>
    where
        T: serde::ser::Serialize,
    {
        Ok(DeliverMany {
            to,
            filterable,
            data: serde_json::to_value(data)?,
        })
    }

    fn apply_filter(rng: &mut ThreadRng, authority: &str, config: &NodeConfig) -> bool {
        let dice = rng.gen::<u8>();

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
        let mut thread_rng = rand::thread_rng();

        for inbox in self.to {
            if self.filterable {
                match inbox.authority_str() {
                    Some(authority) => {
                        if let Ok(node_config) = state.state.node_config.read() {
                            if let Some(cfg) = node_config.get(authority) {
                                if !Self::apply_filter(&mut thread_rng, authority, cfg) {
                                    continue;
                                }
                            }
                        }
                    },
                    None => {} // What the heck?
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

impl ActixJob for DeliverMany {
    type State = JobState;
    type Future = LocalBoxFuture<'static, Result<(), anyhow::Error>>;

    const NAME: &'static str = "relay::jobs::DeliverMany";
    const QUEUE: &'static str = "deliver";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}
