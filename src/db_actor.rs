use crate::{
    db::{add_listener, remove_listener},
    error::MyError,
    label::ArbiterLabel,
};
use activitystreams::primitives::XsdAnyUri;
use actix::prelude::*;
use bb8_postgres::{bb8, tokio_postgres, PostgresConnectionManager};
use log::{error, info};
use tokio::sync::oneshot::{channel, Receiver};

#[derive(Clone)]
pub struct Db {
    actor: Addr<DbActor>,
}

pub type Pool = bb8::Pool<PostgresConnectionManager<tokio_postgres::tls::NoTls>>;

pub enum DbActorState {
    Waiting(tokio_postgres::Config),
    Ready(Pool),
}

pub struct DbActor {
    pool: DbActorState,
}

pub struct DbQuery<F>(pub F);

impl Db {
    pub fn new(config: tokio_postgres::Config) -> Db {
        let actor = Supervisor::start(|_| DbActor {
            pool: DbActorState::new_empty(config),
        });

        Db { actor }
    }

    pub async fn execute_inline<T, F, Fut>(&self, f: F) -> Result<T, MyError>
    where
        T: Send + 'static,
        F: FnOnce(Pool) -> Fut + Send + 'static,
        Fut: Future<Output = T>,
    {
        Ok(self.actor.send(DbQuery(f)).await?.await?)
    }

    pub fn remove_listener(&self, inbox: XsdAnyUri) {
        self.actor.do_send(DbQuery(move |pool: Pool| {
            let inbox = inbox.clone();

            async move {
                let conn = pool.get().await?;

                remove_listener(&conn, &inbox).await.map_err(|e| {
                    error!("Error removing listener, {}", e);
                    e
                })
            }
        }));
    }

    pub fn add_listener(&self, inbox: XsdAnyUri) {
        self.actor.do_send(DbQuery(move |pool: Pool| {
            let inbox = inbox.clone();

            async move {
                let conn = pool.get().await?;

                add_listener(&conn, &inbox).await.map_err(|e| {
                    error!("Error adding listener, {}", e);
                    e
                })
            }
        }));
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
    F: FnOnce(Pool) -> Fut + 'static,
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
    F: FnOnce(Pool) -> Fut,
    Fut: Future<Output = R>,
    R: Send + 'static,
{
    type Result = Receiver<R>;
}
