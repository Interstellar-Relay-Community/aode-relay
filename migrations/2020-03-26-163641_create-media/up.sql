-- Your SQL goes here
CREATE TABLE media (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    media_id UUID UNIQUE NOT NULL,
    url TEXT UNIQUE NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

SELECT diesel_manage_updated_at('media');
