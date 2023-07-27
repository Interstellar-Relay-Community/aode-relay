use http_signature_normalization_actix::{Canceled, Spawn};
use std::{
    panic::AssertUnwindSafe,
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

fn signature_thread(receiver: flume::Receiver<Box<dyn FnOnce() + Send>>, id: usize) {
    let guard = MetricsGuard::guard(id);

    while let Ok(f) = receiver.recv() {
        let start = Instant::now();
        metrics::increment_counter!("relay.signature-thread.operation.start", "id" => id.to_string());
        let res = std::panic::catch_unwind(AssertUnwindSafe(move || {
            (f)();
        }));
        metrics::increment_counter!("relay.signature-thread.operation.end", "complete" => res.is_ok().to_string(), "id" => id.to_string());
        metrics::histogram!("relay.signature-thread.operation.duration", start.elapsed().as_secs_f64(), "id" => id.to_string());

        if let Err(e) = res {
            tracing::warn!("Signature fn panicked: {e:?}");
        }
    }

    guard.disarm();
}

#[derive(Clone, Debug)]
pub(crate) struct Spawner {
    sender: Option<flume::Sender<Box<dyn FnOnce() + Send>>>,
    threads: Option<Arc<Vec<JoinHandle<()>>>>,
}

struct MetricsGuard {
    id: usize,
    start: Instant,
    armed: bool,
}

impl MetricsGuard {
    fn guard(id: usize) -> Self {
        metrics::increment_counter!("relay.signature-thread.launched", "id" => id.to_string());

        Self {
            id,
            start: Instant::now(),
            armed: true,
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for MetricsGuard {
    fn drop(&mut self) {
        metrics::increment_counter!("relay.signature-thread.closed", "clean" => (!self.armed).to_string(), "id" => self.id.to_string());
        metrics::histogram!("relay.signature-thread.duration", self.start.elapsed().as_secs_f64(), "clean" => (!self.armed).to_string(), "id" => self.id.to_string());
        tracing::warn!("Stopping signature thread");
    }
}

impl Spawner {
    pub(crate) fn build(threads: usize) -> std::io::Result<Self> {
        let (sender, receiver) = flume::bounded(8);

        tracing::warn!("Launching {threads} signature threads");

        let threads = (0..threads)
            .map(|i| {
                let receiver = receiver.clone();
                std::thread::Builder::new()
                    .name(format!("signature-thread-{i}"))
                    .spawn(move || {
                        signature_thread(receiver, i);
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Spawner {
            sender: Some(sender),
            threads: Some(Arc::new(threads)),
        })
    }
}

impl Drop for Spawner {
    fn drop(&mut self) {
        self.sender.take();

        if let Some(threads) = self.threads.take().and_then(Arc::into_inner) {
            for thread in threads {
                let _ = thread.join();
            }
        }
    }
}

async fn timer<Fut>(fut: Fut) -> Fut::Output
where
    Fut: std::future::Future,
{
    let id = uuid::Uuid::new_v4();

    metrics::increment_counter!("relay.spawner.wait-timer.start");

    let mut interval = actix_rt::time::interval(Duration::from_secs(5));

    // pass the first tick (instant)
    interval.tick().await;

    let mut fut = std::pin::pin!(fut);

    let mut counter = 0;
    loop {
        tokio::select! {
            out = &mut fut => {
                metrics::increment_counter!("relay.spawner.wait-timer.end");
                return out;
            }
            _ = interval.tick() => {
                counter += 1;
                metrics::increment_counter!("relay.spawner.wait-timer.pending");
                tracing::warn!("Blocking operation {id} is taking a long time, {} seconds", counter * 5);
            }
        }
    }
}

impl Spawn for Spawner {
    type Future<T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, Canceled>>>>;

    fn spawn_blocking<Func, Out>(&self, func: Func) -> Self::Future<Out>
    where
        Func: FnOnce() -> Out + Send + 'static,
        Out: Send + 'static,
    {
        let sender = self.sender.as_ref().expect("Sender exists").clone();

        Box::pin(async move {
            let (tx, rx) = flume::bounded(1);

            let _ = sender
                .send_async(Box::new(move || {
                    if tx.try_send((func)()).is_err() {
                        tracing::warn!("Requestor hung up");
                        metrics::increment_counter!("relay.spawner.disconnected");
                    }
                }))
                .await;

            timer(rx.recv_async()).await.map_err(|_| Canceled)
        })
    }
}
