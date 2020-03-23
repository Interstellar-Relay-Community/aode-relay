-- Your SQL goes here
CREATE TABLE nodes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    listener_id UUID NOT NULL REFERENCES listeners(id) ON DELETE CASCADE,
    nodeinfo JSONB,
    instance JSONB,
    contact JSONB,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

SELECT diesel_manage_updated_at('nodes');
