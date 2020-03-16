-- This file should undo anything in `up.sql`
DROP TRIGGER IF EXISTS whitelists_notify ON whitelists;
DROP TRIGGER IF EXISTS blocks_notify ON blocks;
DROP TRIGGER IF EXISTS listeners_notify ON listeners;

DROP FUNCTION IF EXISTS invoke_whitelists_trigger();
DROP FUNCTION IF EXISTS invoke_blocks_trigger();
DROP FUNCTION IF EXISTS invoke_listeners_trigger();
