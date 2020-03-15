table! {
    blocks (id) {
        id -> Uuid,
        actor_id -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

table! {
    listeners (id) {
        id -> Uuid,
        actor_id -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

table! {
    whitelists (id) {
        id -> Uuid,
        actor_id -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

allow_tables_to_appear_in_same_query!(
    blocks,
    listeners,
    whitelists,
);
