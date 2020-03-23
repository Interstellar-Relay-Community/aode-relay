mod deliver;
mod deliver_many;
mod instance;
mod nodeinfo;
mod process_listeners;
mod storage;
pub use self::{
    deliver::Deliver, deliver_many::DeliverMany, instance::QueryInstance, nodeinfo::QueryNodeinfo,
};

use crate::{
    data::{ActorCache, NodeCache, State},
    db::Db,
    error::MyError,
    jobs::{
        deliver::DeliverProcessor,
        deliver_many::DeliverManyProcessor,
        instance::InstanceProcessor,
        nodeinfo::NodeinfoProcessor,
        process_listeners::{Listeners, ListenersProcessor},
        storage::Storage,
    },
    requests::Requests,
};
use background_jobs::{memory_storage::Storage as MemoryStorage, Job, QueueHandle, WorkerConfig};
use std::time::Duration;

pub fn create_server(db: Db) -> JobServer {
    let local = background_jobs::create_server(MemoryStorage::new());
    let shared = background_jobs::create_server(Storage::new(db));

    local.every(Duration::from_secs(60 * 5), Listeners);

    JobServer::new(shared, local)
}

pub fn create_workers(state: State, actors: ActorCache, job_server: JobServer) {
    let state2 = state.clone();
    let actors2 = actors.clone();
    let job_server2 = job_server.clone();

    let remote_handle = job_server.remote.clone();
    let local_handle = job_server.local.clone();

    WorkerConfig::new(move || JobState::new(state.clone(), actors.clone(), job_server.clone()))
        .register(DeliverProcessor)
        .register(DeliverManyProcessor)
        .set_processor_count("default", 4)
        .start(remote_handle);

    WorkerConfig::new(move || JobState::new(state2.clone(), actors2.clone(), job_server2.clone()))
        .register(NodeinfoProcessor)
        .register(InstanceProcessor)
        .register(ListenersProcessor)
        .set_processor_count("default", 4)
        .start(local_handle);
}

#[derive(Clone)]
pub struct JobState {
    requests: Requests,
    state: State,
    actors: ActorCache,
    node_cache: NodeCache,
    job_server: JobServer,
}

#[derive(Clone)]
pub struct JobServer {
    remote: QueueHandle,
    local: QueueHandle,
}

impl JobState {
    fn new(state: State, actors: ActorCache, job_server: JobServer) -> Self {
        JobState {
            requests: state.requests(),
            node_cache: state.node_cache(),
            actors,
            state,
            job_server,
        }
    }
}

impl JobServer {
    fn new(remote_handle: QueueHandle, local_handle: QueueHandle) -> Self {
        JobServer {
            remote: remote_handle,
            local: local_handle,
        }
    }

    pub fn queue<J>(&self, job: J) -> Result<(), MyError>
    where
        J: Job,
    {
        self.remote.queue(job).map_err(MyError::Queue)
    }

    pub fn queue_local<J>(&self, job: J) -> Result<(), MyError>
    where
        J: Job,
    {
        self.local.queue(job).map_err(MyError::Queue)
    }
}
