-- Your SQL goes here
CREATE OR REPLACE FUNCTION invoke_nodes_trigger ()
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
        channel := 'new_nodes';
        payload := NEW.listener_id;
    WHEN 'UPDATE' THEN
        rec := NEW;
        channel := 'new_nodes';
        payload := NEW.listener_id;
    WHEN 'DELETE' THEN
        rec := OLD;
        channel := 'rm_nodes';
        payload := OLD.listener_id;
    ELSE
        RAISE EXCEPTION 'Unknown TG_OP: "%". Should not occur!', TG_OP;
    END CASE;

    PERFORM pg_notify(channel, payload::TEXT);
    RETURN rec;
END;
$$;

CREATE TRIGGER nodes_notify
    AFTER INSERT OR UPDATE OR DELETE
    ON nodes
FOR EACH ROW
    EXECUTE PROCEDURE invoke_nodes_trigger();
