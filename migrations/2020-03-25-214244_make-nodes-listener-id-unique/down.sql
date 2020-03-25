-- This file should undo anything in `up.sql`
ALTER TABLE nodes DROP CONSTRAINT nodes_listener_ids_unique;
