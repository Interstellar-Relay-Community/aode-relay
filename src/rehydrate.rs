use crate::{db::Db, state::State};
use actix::{
    clock::{interval_at, Duration, Instant},
    Arbiter,
};
use log::error;

pub fn spawn(db: Db, state: State) {
    Arbiter::spawn(async move {
        let start = Instant::now();
        let duration = Duration::from_secs(60 * 10);

        let mut interval = interval_at(start, duration);

        loop {
            interval.tick().await;

            match state.rehydrate(&db).await {
                Err(e) => error!("Error rehydrating, {}", e),
                _ => (),
            }
        }
    });
}
