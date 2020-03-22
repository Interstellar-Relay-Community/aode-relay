-- Your SQL goes here
CREATE TABLE jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id UUID UNIQUE NOT NULL,
    job_queue TEXT NOT NULL,
    job_timeout BIGINT NOT NULL,
    job_updated TIMESTAMP NOT NULL,
    job_status TEXT NOT NULL,
    job_value JSONB NOT NULL,
    job_next_run TIMESTAMP,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX jobs_queue_status_index ON jobs(job_queue, job_status);

SELECT diesel_manage_updated_at('jobs');
