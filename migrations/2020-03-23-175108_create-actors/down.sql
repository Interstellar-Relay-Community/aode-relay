-- This file should undo anything in `up.sql`
DROP TRIGGER IF EXISTS actors_notify ON actors;
DROP FUNCTION IF EXISTS invoke_actors_trigger();
DROP TABLE actors;
