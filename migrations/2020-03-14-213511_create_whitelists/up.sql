-- Your SQL goes here
CREATE TABLE whitelists (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    domain_name TEXT UNIQUE NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP
);

CREATE INDEX whitelists_domain_name_index ON whitelists(domain_name);

SELECT diesel_manage_updated_at('whitelists');
