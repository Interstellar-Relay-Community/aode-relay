table! {
    blocks (id) {
        id -> Uuid,
        domain_name -> Text,
        created_at -> Timestamp,
        updated_at -> Nullable<Timestamp>,
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
    whitelists (id) {
        id -> Uuid,
        domain_name -> Text,
        created_at -> Timestamp,
        updated_at -> Nullable<Timestamp>,
    }
}

allow_tables_to_appear_in_same_query!(
    blocks,
    listeners,
    whitelists,
);
