use activitystreams::primitives::XsdAnyUri;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::RwLock;

pub type ListenersCache = Arc<RwLock<HashSet<XsdAnyUri>>>;

#[derive(Clone)]
pub struct NodeCache {
    listeners: ListenersCache,
    nodes: Arc<RwLock<HashMap<XsdAnyUri, Node>>>,
}

impl NodeCache {
    pub fn new(listeners: ListenersCache) -> Self {
        NodeCache {
            listeners,
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn nodes(&self) -> Vec<Node> {
        let listeners: HashSet<_> = self.listeners.read().await.clone();

        self.nodes
            .read()
            .await
            .iter()
            .filter_map(|(k, v)| {
                if listeners.contains(k) {
                    Some(v.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn set_info(
        &self,
        listener: XsdAnyUri,
        software: String,
        version: String,
        reg: bool,
    ) {
        if !self.listeners.read().await.contains(&listener) {
            let mut nodes = self.nodes.write().await;
            nodes.remove(&listener);
            return;
        }

        let mut write_guard = self.nodes.write().await;
        let node = write_guard
            .entry(listener.clone())
            .or_insert(Node::new(listener));
        node.set_info(software, version, reg);
    }

    pub async fn set_instance(
        &self,
        listener: XsdAnyUri,
        title: String,
        description: String,
        version: String,
        reg: bool,
        requires_approval: bool,
    ) {
        if !self.listeners.read().await.contains(&listener) {
            let mut nodes = self.nodes.write().await;
            nodes.remove(&listener);
            return;
        }

        let mut write_guard = self.nodes.write().await;
        let node = write_guard
            .entry(listener.clone())
            .or_insert(Node::new(listener));
        node.set_instance(title, description, version, reg, requires_approval);
    }

    pub async fn set_contact(
        &self,
        listener: XsdAnyUri,
        username: String,
        display_name: String,
        url: XsdAnyUri,
        avatar: XsdAnyUri,
    ) {
        if !self.listeners.read().await.contains(&listener) {
            let mut nodes = self.nodes.write().await;
            nodes.remove(&listener);
            return;
        }

        let mut write_guard = self.nodes.write().await;
        let node = write_guard
            .entry(listener.clone())
            .or_insert(Node::new(listener));
        node.set_contact(username, display_name, url, avatar);
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub base: XsdAnyUri,
    pub info: Option<Info>,
    pub instance: Option<Instance>,
    pub contact: Option<Contact>,
}

impl Node {
    pub fn new(mut uri: XsdAnyUri) -> Self {
        let url = uri.as_mut();
        url.set_fragment(None);
        url.set_query(None);
        url.set_path("");

        Node {
            base: uri,
            info: None,
            instance: None,
            contact: None,
        }
    }

    fn set_info(&mut self, software: String, version: String, reg: bool) -> &mut Self {
        self.info = Some(Info {
            software,
            version,
            reg,
        });
        self
    }

    fn set_instance(
        &mut self,
        title: String,
        description: String,
        version: String,
        reg: bool,
        requires_approval: bool,
    ) -> &mut Self {
        self.instance = Some(Instance {
            title,
            description,
            version,
            reg,
            requires_approval,
        });
        self
    }

    fn set_contact(
        &mut self,
        username: String,
        display_name: String,
        url: XsdAnyUri,
        avatar: XsdAnyUri,
    ) -> &mut Self {
        self.contact = Some(Contact {
            username,
            display_name,
            url,
            avatar,
        });
        self
    }
}

#[derive(Clone, Debug)]
pub struct Info {
    pub software: String,
    pub version: String,
    pub reg: bool,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub title: String,
    pub description: String,
    pub version: String,
    pub reg: bool,
    pub requires_approval: bool,
}

#[derive(Clone, Debug)]
pub struct Contact {
    pub username: String,
    pub display_name: String,
    pub url: XsdAnyUri,
    pub avatar: XsdAnyUri,
}
