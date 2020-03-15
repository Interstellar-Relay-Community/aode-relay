-- Your SQL goes here
CREATE TABLE listeners (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id TEXT UNIQUE NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);

CREATE INDEX listeners_actor_id_index ON listeners(actor_id);

SELECT diesel_manage_updated_at('listeners');
