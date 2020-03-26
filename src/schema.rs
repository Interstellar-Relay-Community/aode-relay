table! {
    actors (id) {
        id -> Uuid,
        actor_id -> Text,
        public_key -> Text,
        public_key_id -> Text,
        listener_id -> Uuid,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

table! {
    blocks (id) {
        id -> Uuid,
        domain_name -> Text,
        created_at -> Timestamp,
        updated_at -> Nullable<Timestamp>,
    }
}

table! {
    jobs (id) {
        id -> Uuid,
        job_id -> Uuid,
        job_queue -> Text,
        job_timeout -> Int8,
        job_updated -> Timestamp,
        job_status -> Text,
        job_value -> Jsonb,
        job_next_run -> Nullable<Timestamp>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

table! {
    listeners (id) {
        id -> Uuid,
        actor_id -> Text,
        created_at -> Timestamp,
        updated_at -> Nullable<Timestamp>,
    }
}

table! {
    media (id) {
        id -> Uuid,
        media_id -> Uuid,
        url -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

table! {
    nodes (id) {
        id -> Uuid,
        listener_id -> Uuid,
        nodeinfo -> Nullable<Jsonb>,
        instance -> Nullable<Jsonb>,
        contact -> Nullable<Jsonb>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

table! {
    settings (id) {
        id -> Uuid,
        key -> Text,
        value -> Text,
        created_at -> Timestamp,
        updated_at -> Nullable<Timestamp>,
    }
}

table! {
    whitelists (id) {
        id -> Uuid,
        domain_name -> Text,
        created_at -> Timestamp,
        updated_at -> Nullable<Timestamp>,
    }
}

joinable!(actors -> listeners (listener_id));
joinable!(nodes -> listeners (listener_id));

allow_tables_to_appear_in_same_query!(
    actors,
    blocks,
    jobs,
    listeners,
    media,
    nodes,
    settings,
    whitelists,
);
