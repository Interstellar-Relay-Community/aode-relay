-- Your SQL goes here
CREATE OR REPLACE FUNCTION invoke_listeners_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
AS $$
DECLARE
    rec RECORD;
    channel TEXT;
    payload TEXT;
BEGIN
    case TG_OP
    WHEN 'INSERT' THEN
        rec := NEW;
        channel := 'new_listeners';
        payload := NEW.actor_id;
    WHEN 'DELETE' THEN
        rec := OLD;
        channel := 'rm_listeners';
        payload := OLD.actor_id;
    ELSE
        RAISE EXCEPTION 'Unknown TG_OP: "%". Should not occur!', TG_OP;
    END CASE;

    PERFORM pg_notify(channel, payload::TEXT);
    RETURN rec;
END;
$$;

CREATE OR REPLACE FUNCTION invoke_blocks_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
AS $$
DECLARE
    rec RECORD;
    channel TEXT;
    payload TEXT;
BEGIN
    case TG_OP
    WHEN 'INSERT' THEN
        rec := NEW;
        channel := 'new_blocks';
        payload := NEW.domain_name;
    WHEN 'DELETE' THEN
        rec := OLD;
        channel := 'rm_blocks';
        payload := OLD.domain_name;
    ELSE
        RAISE EXCEPTION 'Unknown TG_OP: "%". Should not occur!', TG_OP;
    END CASE;

    PERFORM pg_notify(channel, payload::TEXT);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION invoke_whitelists_trigger ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
AS $$
DECLARE
    rec RECORD;
    channel TEXT;
    payload TEXT;
BEGIN
    case TG_OP
    WHEN 'INSERT' THEN
        rec := NEW;
        channel := 'new_whitelists';
        payload := NEW.domain_name;
    WHEN 'DELETE' THEN
        rec := OLD;
        channel := 'rm_whitelists';
        payload := OLD.domain_name;
    ELSE
        RAISE EXCEPTION 'Unknown TG_OP: "%". Should not occur!', TG_OP;
    END CASE;

    PERFORM pg_notify(channel, payload::TEXT);
    RETURN rec;
END;
$$;

CREATE TRIGGER listeners_notify
    AFTER INSERT OR UPDATE OR DELETE
    ON listeners
FOR EACH ROW
    EXECUTE PROCEDURE invoke_listeners_trigger();

CREATE TRIGGER blocks_notify
    AFTER INSERT OR UPDATE OR DELETE
    ON blocks
FOR EACH ROW
    EXECUTE PROCEDURE invoke_blocks_trigger();

CREATE TRIGGER whitelists_notify
    AFTER INSERT OR UPDATE OR DELETE
    ON whitelists
FOR EACH ROW
    EXECUTE PROCEDURE invoke_whitelists_trigger();
