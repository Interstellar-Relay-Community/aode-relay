-- Your SQL goes here
ALTER TABLE nodes ADD CONSTRAINT nodes_listener_ids_unique UNIQUE (listener_id);
