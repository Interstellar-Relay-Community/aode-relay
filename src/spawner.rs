use async_cpupool::CpuPool;
use http_signature_normalization_actix::{Canceled, Spawn};
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct Spawner {
    pool: CpuPool,
}

impl Spawner {
    pub(crate) fn build(name: &'static str, threads: u16) -> anyhow::Result<Self> {
        let pool = CpuPool::configure()
            .name(name)
            .max_threads(threads)
            .build()?;

        Ok(Spawner { pool })
    }

    pub(crate) async fn close(self) {
        self.pool.close().await;
    }
}

impl std::fmt::Debug for Spawner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spawner").finish()
    }
}

async fn timer<Fut>(fut: Fut) -> Fut::Output
where
    Fut: std::future::Future,
{
    let id = uuid::Uuid::new_v4();

    metrics::counter!("relay.spawner.wait-timer.start").increment(1);

    let mut interval = tokio::time::interval(Duration::from_secs(5));

    // pass the first tick (instant)
    interval.tick().await;

    let mut fut = std::pin::pin!(fut);

    let mut counter = 0;
    loop {
        tokio::select! {
            out = &mut fut => {
                metrics::counter!("relay.spawner.wait-timer.end").increment(1);
                return out;
            }
            _ = interval.tick() => {
                counter += 1;
                metrics::counter!("relay.spawner.wait-timer.pending").increment(1);
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
        let pool = self.pool.clone();

        Box::pin(async move { timer(pool.spawn(func)).await.map_err(|_| Canceled) })
    }
}

impl http_signature_normalization_reqwest::Spawn for Spawner {
    type Future<T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, http_signature_normalization_reqwest::Canceled>> + Send>> where T: Send;

    fn spawn_blocking<Func, Out>(&self, func: Func) -> Self::Future<Out>
    where
        Func: FnOnce() -> Out + Send + 'static,
        Out: Send + 'static,
    {
        let pool = self.pool.clone();

        Box::pin(async move {
            timer(pool.spawn(func))
                .await
                .map_err(|_| http_signature_normalization_reqwest::Canceled)
        })
    }
}
