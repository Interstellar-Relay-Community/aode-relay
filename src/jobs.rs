pub mod apub;
mod contact;
mod deliver;
mod deliver_many;
mod instance;
mod nodeinfo;
mod process_listeners;
mod record_last_online;

pub(crate) use self::{
    contact::QueryContact, deliver::Deliver, deliver_many::DeliverMany, instance::QueryInstance,
    nodeinfo::QueryNodeinfo,
};

use crate::{
    config::Config,
    data::{ActorCache, MediaCache, NodeCache, State},
    error::{Error, ErrorKind},
    jobs::{process_listeners::Listeners, record_last_online::RecordLastOnline},
    requests::Requests,
};
use background_jobs::{
    memory_storage::{ActixTimer, Storage},
    Job, QueueHandle, WorkerConfig,
};
use std::time::Duration;

fn debug_object(activity: &serde_json::Value) -> &serde_json::Value {
    let mut object = &activity["object"]["type"];

    if object.is_null() {
        object = &activity["object"]["id"];
    }

    if object.is_null() {
        object = &activity["object"];
    }

    object
}

pub(crate) fn create_workers(
    state: State,
    actors: ActorCache,
    media: MediaCache,
    config: Config,
) -> JobServer {
    let queue_handle = WorkerConfig::new(Storage::new(ActixTimer), move |queue_handle| {
        JobState::new(
            state.clone(),
            actors.clone(),
            JobServer::new(queue_handle),
            media.clone(),
            config.clone(),
        )
    })
    .register::<Deliver>()
    .register::<DeliverMany>()
    .register::<QueryNodeinfo>()
    .register::<QueryInstance>()
    .register::<Listeners>()
    .register::<QueryContact>()
    .register::<RecordLastOnline>()
    .register::<apub::Announce>()
    .register::<apub::Follow>()
    .register::<apub::Forward>()
    .register::<apub::Reject>()
    .register::<apub::Undo>()
    .set_worker_count("maintenance", 2)
    .set_worker_count("apub", 2)
    .set_worker_count("deliver", 8)
    .start();

    queue_handle.every(Duration::from_secs(60 * 5), Listeners);
    queue_handle.every(Duration::from_secs(60 * 10), RecordLastOnline);

    JobServer::new(queue_handle)
}

#[derive(Clone, Debug)]
pub(crate) struct JobState {
    requests: Requests,
    state: State,
    actors: ActorCache,
    config: Config,
    media: MediaCache,
    node_cache: NodeCache,
    job_server: JobServer,
}

#[derive(Clone)]
pub(crate) struct JobServer {
    remote: QueueHandle,
}

impl std::fmt::Debug for JobServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobServer")
            .field("queue_handle", &"QueueHandle")
            .finish()
    }
}

impl JobState {
    fn new(
        state: State,
        actors: ActorCache,
        job_server: JobServer,
        media: MediaCache,
        config: Config,
    ) -> Self {
        JobState {
            requests: state.requests(&config),
            node_cache: state.node_cache(),
            actors,
            config,
            media,
            state,
            job_server,
        }
    }
}

impl JobServer {
    fn new(remote_handle: QueueHandle) -> Self {
        JobServer {
            remote: remote_handle,
        }
    }

    pub(crate) async fn queue<J>(&self, job: J) -> Result<(), Error>
    where
        J: Job,
    {
        self.remote
            .queue(job)
            .await
            .map_err(ErrorKind::Queue)
            .map_err(Into::into)
    }
}

struct Boolish {
    inner: bool,
}

impl std::ops::Deref for Boolish {
    type Target = bool;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'de> serde::Deserialize<'de> for Boolish {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum BoolThing {
            Bool(bool),
            String(String),
        }

        let thing: BoolThing = serde::Deserialize::deserialize(deserializer)?;

        match thing {
            BoolThing::Bool(inner) => Ok(Boolish { inner }),
            BoolThing::String(s) if s.to_lowercase() == "false" => Ok(Boolish { inner: false }),
            BoolThing::String(_) => Ok(Boolish { inner: true }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Boolish;

    #[test]
    fn boolish_works() {
        const CASES: &[(&str, bool)] = &[
            ("false", false),
            ("\"false\"", false),
            ("\"FALSE\"", false),
            ("true", true),
            ("\"true\"", true),
            ("\"anything else\"", true),
        ];

        for (case, output) in CASES {
            let b: Boolish = serde_json::from_str(case).unwrap();
            assert_eq!(*b, *output);
        }
    }
}
