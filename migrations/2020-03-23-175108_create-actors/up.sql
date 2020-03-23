-- Your SQL goes here
CREATE TABLE actors (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id TEXT UNIQUE NOT NULL,
    public_key TEXT NOT NULL,
    public_key_id TEXT UNIQUE NOT NULL,
    listener_id UUID NOT NULL REFERENCES listeners(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

SELECT diesel_manage_updated_at('actors');

CREATE OR REPLACE FUNCTION invoke_actors_trigger ()
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
        channel := 'new_actors';
        payload := NEW.actor_id;
    WHEN 'UPDATE' THEN
        rec := NEW;
        channel := 'new_actors';
        payload := NEW.actor_id;
    WHEN 'DELETE' THEN
        rec := OLD;
        channel := 'rm_actors';
        payload := OLD.actor_id;
    ELSE
        RAISE EXCEPTION 'Unknown TG_OP: "%". Should not occur!', TG_OP;
    END CASE;

    PERFORM pg_notify(channel, payload::TEXT);
    RETURN rec;
END;
$$;

CREATE TRIGGER actors_notify
    AFTER INSERT OR UPDATE OR DELETE
    ON actors
FOR EACH ROW
    EXECUTE PROCEDURE invoke_actors_trigger();
