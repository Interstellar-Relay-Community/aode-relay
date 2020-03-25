-- This file should undo anything in `up.sql`
DROP TRIGGER IF EXISTS nodes_notify ON nodes;
DROP FUNCTION IF EXISTS invoke_nodes_trigger();
