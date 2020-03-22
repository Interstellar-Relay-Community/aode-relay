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

allow_tables_to_appear_in_same_query!(
    blocks,
    jobs,
    listeners,
    settings,
    whitelists,
);
