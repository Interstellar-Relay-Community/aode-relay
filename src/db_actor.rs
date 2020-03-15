use crate::label::ArbiterLabel;
use actix::prelude::*;
use bb8_postgres::{bb8, tokio_postgres, PostgresConnectionManager};
use log::{error, info};
use tokio::sync::oneshot::{channel, Receiver};

pub type Pool = bb8::Pool<PostgresConnectionManager<tokio_postgres::tls::NoTls>>;

pub enum DbActorState {
    Waiting(tokio_postgres::Config),
    Ready(Pool),
}

pub struct DbActor {
    pool: DbActorState,
}

pub struct DbQuery<F>(pub F);

impl DbActor {
    pub fn new(config: tokio_postgres::Config) -> Addr<Self> {
        Supervisor::start(|_| DbActor {
            pool: DbActorState::new_empty(config),
        })
    }
}

impl DbActorState {
    pub fn new_empty(config: tokio_postgres::Config) -> Self {
        DbActorState::Waiting(config)
    }

    pub async fn new(config: tokio_postgres::Config) -> Result<Self, tokio_postgres::error::Error> {
        let manager = PostgresConnectionManager::new(config, tokio_postgres::tls::NoTls);
        let pool = bb8::Pool::builder().max_size(8).build(manager).await?;

        Ok(DbActorState::Ready(pool))
    }
}

impl Actor for DbActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Starting DB Actor in {}", ArbiterLabel::get());
        match self.pool {
            DbActorState::Waiting(ref config) => {
                let fut =
                    DbActorState::new(config.clone())
                        .into_actor(self)
                        .map(|res, actor, ctx| {
                            match res {
                                Ok(pool) => {
                                    info!("DB pool created in {}", ArbiterLabel::get());
                                    actor.pool = pool;
                                }
                                Err(e) => {
                                    error!(
                                        "Error starting DB Actor in {}, {}",
                                        ArbiterLabel::get(),
                                        e
                                    );
                                    ctx.stop();
                                }
                            };
                        });

                ctx.wait(fut);
            }
            _ => (),
        };
    }
}

impl Supervised for DbActor {}

impl<F, Fut, R> Handler<DbQuery<F>> for DbActor
where
    F: Fn(Pool) -> Fut + 'static,
    Fut: Future<Output = R>,
    R: Send + 'static,
{
    type Result = ResponseFuture<Receiver<R>>;

    fn handle(&mut self, msg: DbQuery<F>, ctx: &mut Self::Context) -> Self::Result {
        let (tx, rx) = channel();

        let pool = match self.pool {
            DbActorState::Ready(ref pool) => pool.clone(),
            _ => {
                error!("Tried to query DB before ready");
                return Box::pin(async move { rx });
            }
        };

        ctx.spawn(
            async move {
                let result = (msg.0)(pool).await;
                let _ = tx.send(result);
            }
            .into_actor(self),
        );

        Box::pin(async move { rx })
    }
}

impl<F, Fut, R> Message for DbQuery<F>
where
    F: Fn(Pool) -> Fut,
    Fut: Future<Output = R>,
    R: Send + 'static,
{
    type Result = Receiver<R>;
}
