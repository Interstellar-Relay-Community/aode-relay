use crate::{db::Db, error::MyError};
use background_jobs_core::{JobInfo, Stats};
use bb8_postgres::tokio_postgres::types::Json;
use log::debug;
use uuid::Uuid;

#[derive(Clone)]
pub struct Storage {
    db: Db,
}

impl Storage {
    pub fn new(db: Db) -> Self {
        Storage { db }
    }
}

#[async_trait::async_trait]
impl background_jobs_core::Storage for Storage {
    type Error = MyError;

    async fn generate_id(&self) -> Result<Uuid, MyError> {
        // TODO: Ensure unique job id
        Ok(Uuid::new_v4())
    }

    async fn save_job(&self, job: JobInfo) -> Result<(), MyError> {
        debug!(
            "Inserting job {} status {} for queue {}",
            job.id(),
            job.status(),
            job.queue()
        );
        self.db.pool().get().await?.execute(
            "INSERT INTO jobs
                (job_id, job_queue, job_timeout, job_updated, job_status, job_next_run, job_value, created_at)
             VALUES
                ($1::UUID, $2::TEXT, $3::BIGINT, $4::TIMESTAMP, $5::TEXT, $6::TIMESTAMP, $7::JSONB, 'now')
             ON CONFLICT (job_id)
             DO UPDATE SET
                job_updated = $4::TIMESTAMP,
                job_status = $5::TEXT,
                job_next_run = $6::TIMESTAMP,
                job_value = $7::JSONB;",
            &[&job.id(), &job.queue(), &job.timeout(), &job.updated_at().naive_utc(), &job.status().to_string(), &job.next_queue().map(|q| q.naive_utc()), &Json(&job)],
        )
        .await?;

        Ok(())
    }

    async fn fetch_job(&self, id: Uuid) -> Result<Option<JobInfo>, MyError> {
        debug!(
            "SELECT job_value FROM jobs WHERE job_id = $1::UUID LIMIT 1; [{}]",
            id
        );
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT job_value
                 FROM jobs
                 WHERE job_id = $1::UUID
                 LIMIT 1;",
                &[&id],
            )
            .await?;

        let row = if let Some(row) = row_opt {
            row
        } else {
            return Ok(None);
        };

        let value: Json<JobInfo> = row.try_get(0)?;

        Ok(Some(value.0))
    }

    async fn fetch_job_from_queue(&self, queue: &str) -> Result<Option<JobInfo>, MyError> {
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "UPDATE jobs
                 SET
                    job_status = 'Running',
                    job_updated = 'now'
                 WHERE
                    job_id = (
                        SELECT job_id
                        FROM jobs
                        WHERE
                            job_queue = $1::TEXT
                        AND
                            (
                                job_next_run IS NULL
                            OR
                                job_next_run < now()
                            )
                        AND
                            (
                                job_status = 'Pending'
                            OR
                                (
                                    job_status = 'Running'
                                AND
                                    NOW() > (INTERVAL '1 millisecond' * job_timeout + job_updated)
                                )
                            )
                        LIMIT 1
                        FOR UPDATE SKIP LOCKED
                    )
                 RETURNING job_value;",
                &[&queue],
            )
            .await?;

        let row = if let Some(row) = row_opt {
            row
        } else {
            return Ok(None);
        };

        let value: Json<JobInfo> = row.try_get(0)?;
        let job = value.0;

        debug!("Found job {} in queue {}", job.id(), queue);

        Ok(Some(job))
    }

    async fn queue_job(&self, _queue: &str, _id: Uuid) -> Result<(), MyError> {
        // Queue Job is a no-op, since jobs are always in their queue
        Ok(())
    }

    async fn run_job(&self, _id: Uuid, _runner_id: Uuid) -> Result<(), MyError> {
        // Run Job is a no-op, since jobs are marked running at fetch
        Ok(())
    }

    async fn delete_job(&self, id: Uuid) -> Result<(), MyError> {
        debug!("Deleting job {}", id);
        self.db
            .pool()
            .get()
            .await?
            .execute("DELETE FROM jobs WHERE job_id = $1::UUID;", &[&id])
            .await?;

        Ok(())
    }

    async fn get_stats(&self) -> Result<Stats, MyError> {
        // TODO: Stats are unimplemented
        Ok(Stats::default())
    }

    async fn update_stats<F>(&self, _f: F) -> Result<(), MyError>
    where
        F: Fn(Stats) -> Stats + Send,
    {
        // TODO: Stats are unimplemented
        Ok(())
    }
}
