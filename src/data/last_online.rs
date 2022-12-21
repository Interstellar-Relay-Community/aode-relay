use activitystreams::iri_string::types::IriStr;
use std::{collections::HashMap, sync::Mutex};
use time::OffsetDateTime;

pub(crate) struct LastOnline {
    domains: Mutex<HashMap<String, OffsetDateTime>>,
}

impl LastOnline {
    pub(crate) fn mark_seen(&self, iri: &IriStr) {
        if let Some(authority) = iri.authority_str() {
            self.domains
                .lock()
                .unwrap()
                .insert(authority.to_string(), OffsetDateTime::now_utc());
        }
    }

    pub(crate) fn take(&self) -> HashMap<String, OffsetDateTime> {
        std::mem::take(&mut *self.domains.lock().unwrap())
    }

    pub(crate) fn empty() -> Self {
        Self {
            domains: Mutex::new(HashMap::default()),
        }
    }
}
