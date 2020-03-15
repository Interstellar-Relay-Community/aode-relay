use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone, Debug)]
pub struct ArbiterLabelFactory(Arc<AtomicUsize>);

#[derive(Clone, Debug)]
pub struct ArbiterLabel(usize);

impl ArbiterLabelFactory {
    pub fn new() -> Self {
        ArbiterLabelFactory(Arc::new(AtomicUsize::new(0)))
    }

    pub fn set_label(&self) {
        if !actix::Arbiter::contains_item::<ArbiterLabel>() {
            let id = self.0.fetch_add(1, Ordering::SeqCst);
            actix::Arbiter::set_item(ArbiterLabel(id));
        }
    }
}

impl ArbiterLabel {
    pub fn get() -> ArbiterLabel {
        actix::Arbiter::get_item(|label: &ArbiterLabel| label.clone())
    }
}

impl std::fmt::Display for ArbiterLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Arbiter #{}", self.0)
    }
}
