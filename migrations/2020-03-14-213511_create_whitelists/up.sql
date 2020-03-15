-- Your SQL goes here
CREATE TABLE whitelists (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id TEXT UNIQUE NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE INDEX whitelists_actor_id_index ON whitelists(actor_id);

SELECT diesel_manage_updated_at('whitelists');
