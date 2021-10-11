pub mod apub;
mod cache_media;
mod contact;
mod deliver;
mod deliver_many;
mod instance;
mod nodeinfo;
mod process_listeners;

pub(crate) use self::{
    cache_media::CacheMedia, contact::QueryContact, deliver::Deliver, deliver_many::DeliverMany,
    instance::QueryInstance, nodeinfo::QueryNodeinfo,
};

use crate::{
    config::Config,
    data::{ActorCache, MediaCache, NodeCache, State},
    db::Db,
    error::{Error, ErrorKind},
    jobs::process_listeners::Listeners,
    requests::Requests,
};
use background_jobs::{memory_storage::Storage, Job, QueueHandle, WorkerConfig};
use std::time::Duration;

pub(crate) fn create_server() -> JobServer {
    let shared = background_jobs::create_server(Storage::new());

    shared.every(Duration::from_secs(60 * 5), Listeners);

    JobServer::new(shared)
}

pub(crate) fn create_workers(
    db: Db,
    state: State,
    actors: ActorCache,
    job_server: JobServer,
    media: MediaCache,
    config: Config,
) {
    let remote_handle = job_server.remote.clone();

    WorkerConfig::new(move || {
        JobState::new(
            db.clone(),
            state.clone(),
            actors.clone(),
            job_server.clone(),
            media.clone(),
            config.clone(),
        )
    })
    .register::<Deliver>()
    .register::<DeliverMany>()
    .register::<QueryNodeinfo>()
    .register::<QueryInstance>()
    .register::<Listeners>()
    .register::<CacheMedia>()
    .register::<QueryContact>()
    .register::<apub::Announce>()
    .register::<apub::Follow>()
    .register::<apub::Forward>()
    .register::<apub::Reject>()
    .register::<apub::Undo>()
    .set_worker_count("default", 16)
    .start(remote_handle);
}

#[derive(Clone, Debug)]
pub(crate) struct JobState {
    db: Db,
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
        db: Db,
        state: State,
        actors: ActorCache,
        job_server: JobServer,
        media: MediaCache,
        config: Config,
    ) -> Self {
        JobState {
            requests: state.requests(&config),
            node_cache: state.node_cache(),
            db,
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
