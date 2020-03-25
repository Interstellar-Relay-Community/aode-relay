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
use background_jobs::{Job, QueueHandle, WorkerConfig};
use std::time::Duration;

pub fn create_server(db: Db) -> JobServer {
    let shared = background_jobs::create_server(Storage::new(db));

    shared.every(Duration::from_secs(60 * 5), Listeners);

    JobServer::new(shared)
}

pub fn create_workers(state: State, actors: ActorCache, job_server: JobServer) {
    let remote_handle = job_server.remote.clone();

    WorkerConfig::new(move || JobState::new(state.clone(), actors.clone(), job_server.clone()))
        .register(DeliverProcessor)
        .register(DeliverManyProcessor)
        .register(NodeinfoProcessor)
        .register(InstanceProcessor)
        .register(ListenersProcessor)
        .set_processor_count("default", 4)
        .start(remote_handle);
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
    fn new(remote_handle: QueueHandle) -> Self {
        JobServer {
            remote: remote_handle,
        }
    }

    pub fn queue<J>(&self, job: J) -> Result<(), MyError>
    where
        J: Job,
    {
        self.remote.queue(job).map_err(MyError::Queue)
    }
}
