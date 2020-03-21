mod deliver;
mod deliver_many;
pub use self::{deliver::Deliver, deliver_many::DeliverMany};

use crate::{
    error::MyError,
    jobs::{deliver::DeliverProcessor, deliver_many::DeliverManyProcessor},
    requests::Requests,
    state::State,
};
use background_jobs::{memory_storage::Storage, Job, QueueHandle, WorkerConfig};

pub fn create_server() -> JobServer {
    JobServer::new(background_jobs::create_server(Storage::new()))
}

pub fn create_workers(state: State, job_server: JobServer) {
    let queue_handle = job_server.queue_handle();

    WorkerConfig::new(move || JobState::new(state.requests(), job_server.clone()))
        .register(DeliverProcessor)
        .register(DeliverManyProcessor)
        .set_processor_count("default", 4)
        .start(queue_handle);
}

#[derive(Clone)]
pub struct JobState {
    requests: Requests,
    job_server: JobServer,
}

#[derive(Clone)]
pub struct JobServer {
    inner: QueueHandle,
}

impl JobState {
    fn new(requests: Requests, job_server: JobServer) -> Self {
        JobState {
            requests,
            job_server,
        }
    }
}

impl JobServer {
    fn new(queue_handle: QueueHandle) -> Self {
        JobServer {
            inner: queue_handle,
        }
    }

    pub fn queue_handle(&self) -> QueueHandle {
        self.inner.clone()
    }

    pub fn queue<J>(&self, job: J) -> Result<(), MyError>
    where
        J: Job,
    {
        self.inner.queue(job).map_err(MyError::Queue)
    }
}
