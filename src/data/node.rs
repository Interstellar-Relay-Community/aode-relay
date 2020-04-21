use crate::{db::Db, error::MyError};
use activitystreams::primitives::XsdAnyUri;
use log::{debug, error};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::RwLock;
use tokio_postgres::types::Json;
use uuid::Uuid;

pub type ListenersCache = Arc<RwLock<HashSet<XsdAnyUri>>>;

#[derive(Clone)]
pub struct NodeCache {
    db: Db,
    listeners: ListenersCache,
    nodes: Arc<RwLock<HashMap<XsdAnyUri, Node>>>,
}

impl NodeCache {
    pub fn new(db: Db, listeners: ListenersCache) -> Self {
        NodeCache {
            db,
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

    pub async fn is_nodeinfo_outdated(&self, listener: &XsdAnyUri) -> bool {
        let read_guard = self.nodes.read().await;

        let node = match read_guard.get(listener) {
            None => {
                debug!("No node for listener {}", listener);
                return true;
            }
            Some(node) => node,
        };

        match node.info.as_ref() {
            Some(nodeinfo) => nodeinfo.outdated(),
            None => {
                debug!("No info for node {}", node.base);
                true
            }
        }
    }

    pub async fn is_contact_outdated(&self, listener: &XsdAnyUri) -> bool {
        let read_guard = self.nodes.read().await;

        let node = match read_guard.get(listener) {
            None => {
                debug!("No node for listener {}", listener);
                return true;
            }
            Some(node) => node,
        };

        match node.contact.as_ref() {
            Some(contact) => contact.outdated(),
            None => {
                debug!("No contact for node {}", node.base);
                true
            }
        }
    }

    pub async fn is_instance_outdated(&self, listener: &XsdAnyUri) -> bool {
        let read_guard = self.nodes.read().await;

        let node = match read_guard.get(listener) {
            None => {
                debug!("No node for listener {}", listener);
                return true;
            }
            Some(node) => node,
        };

        match node.instance.as_ref() {
            Some(instance) => instance.outdated(),
            None => {
                debug!("No instance for node {}", node.base);
                true
            }
        }
    }

    pub async fn cache_by_id(&self, id: Uuid) {
        if let Err(e) = self.do_cache_by_id(id).await {
            error!("Error loading node into cache, {}", e);
        }
    }

    pub async fn bust_by_id(&self, id: Uuid) {
        if let Err(e) = self.do_bust_by_id(id).await {
            error!("Error busting node cache, {}", e);
        }
    }

    async fn do_bust_by_id(&self, id: Uuid) -> Result<(), MyError> {
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT ls.actor_id
                 FROM listeners AS ls
                 INNER JOIN nodes AS nd ON nd.listener_id = ls.id
                 WHERE nd.id = $1::UUID
                 LIMIT 1;",
                &[&id],
            )
            .await?;

        let row = if let Some(row) = row_opt {
            row
        } else {
            return Ok(());
        };

        let listener: String = row.try_get(0)?;
        let listener: XsdAnyUri = listener.parse()?;

        self.nodes.write().await.remove(&listener);

        Ok(())
    }

    async fn do_cache_by_id(&self, id: Uuid) -> Result<(), MyError> {
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT ls.actor_id, nd.nodeinfo, nd.instance, nd.contact
                 FROM nodes AS nd
                 INNER JOIN listeners AS ls ON nd.listener_id = ls.id
                 WHERE nd.id = $1::UUID
                 LIMIT 1;",
                &[&id],
            )
            .await?;

        let row = if let Some(row) = row_opt {
            row
        } else {
            return Ok(());
        };

        let listener: String = row.try_get(0)?;
        let listener: XsdAnyUri = listener.parse()?;
        let info: Option<Json<Info>> = row.try_get(1)?;
        let instance: Option<Json<Instance>> = row.try_get(2)?;
        let contact: Option<Json<Contact>> = row.try_get(3)?;

        {
            let mut write_guard = self.nodes.write().await;
            let node = write_guard
                .entry(listener.clone())
                .or_insert(Node::new(listener));

            if let Some(info) = info {
                node.info = Some(info.0);
            }
            if let Some(instance) = instance {
                node.instance = Some(instance.0);
            }
            if let Some(contact) = contact {
                node.contact = Some(contact.0);
            }
        }

        Ok(())
    }

    pub async fn set_info(
        &self,
        listener: &XsdAnyUri,
        software: String,
        version: String,
        reg: bool,
    ) -> Result<(), MyError> {
        if !self.listeners.read().await.contains(listener) {
            let mut nodes = self.nodes.write().await;
            nodes.remove(listener);
            return Ok(());
        }

        let node = {
            let mut write_guard = self.nodes.write().await;
            let node = write_guard
                .entry(listener.clone())
                .or_insert(Node::new(listener.clone()));
            node.set_info(software, version, reg);
            node.clone()
        };
        self.save(listener, &node).await?;
        Ok(())
    }

    pub async fn set_instance(
        &self,
        listener: &XsdAnyUri,
        title: String,
        description: String,
        version: String,
        reg: bool,
        requires_approval: bool,
    ) -> Result<(), MyError> {
        if !self.listeners.read().await.contains(listener) {
            let mut nodes = self.nodes.write().await;
            nodes.remove(listener);
            return Ok(());
        }

        let node = {
            let mut write_guard = self.nodes.write().await;
            let node = write_guard
                .entry(listener.clone())
                .or_insert(Node::new(listener.clone()));
            node.set_instance(title, description, version, reg, requires_approval);
            node.clone()
        };
        self.save(listener, &node).await?;
        Ok(())
    }

    pub async fn set_contact(
        &self,
        listener: &XsdAnyUri,
        username: String,
        display_name: String,
        url: XsdAnyUri,
        avatar: XsdAnyUri,
    ) -> Result<(), MyError> {
        if !self.listeners.read().await.contains(listener) {
            let mut nodes = self.nodes.write().await;
            nodes.remove(listener);
            return Ok(());
        }

        let node = {
            let mut write_guard = self.nodes.write().await;
            let node = write_guard
                .entry(listener.clone())
                .or_insert(Node::new(listener.clone()));
            node.set_contact(username, display_name, url, avatar);
            node.clone()
        };
        self.save(listener, &node).await?;
        Ok(())
    }

    pub async fn save(&self, listener: &XsdAnyUri, node: &Node) -> Result<(), MyError> {
        let row_opt = self
            .db
            .pool()
            .get()
            .await?
            .query_opt(
                "SELECT id FROM listeners WHERE actor_id = $1::TEXT LIMIT 1;",
                &[&listener.as_str()],
            )
            .await?;

        let id: Uuid = if let Some(row) = row_opt {
            row.try_get(0)?
        } else {
            return Err(MyError::NotSubscribed(listener.as_str().to_owned()));
        };

        self.db
            .pool()
            .get()
            .await?
            .execute(
                "INSERT INTO nodes (
                listener_id,
                nodeinfo,
                instance,
                contact,
                created_at,
                updated_at
             ) VALUES (
                $1::UUID,
                $2::JSONB,
                $3::JSONB,
                $4::JSONB,
                'now',
                'now'
             ) ON CONFLICT (listener_id)
             DO UPDATE SET
                nodeinfo = $2::JSONB,
                instance = $3::JSONB,
                contact = $4::JSONB;",
                &[
                    &id,
                    &Json(&node.info),
                    &Json(&node.instance),
                    &Json(&node.contact),
                ],
            )
            .await?;
        Ok(())
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
            updated: SystemTime::now(),
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
            updated: SystemTime::now(),
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
            updated: SystemTime::now(),
        });
        self
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Info {
    pub software: String,
    pub version: String,
    pub reg: bool,
    pub updated: SystemTime,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Instance {
    pub title: String,
    pub description: String,
    pub version: String,
    pub reg: bool,
    pub requires_approval: bool,
    pub updated: SystemTime,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Contact {
    pub username: String,
    pub display_name: String,
    pub url: XsdAnyUri,
    pub avatar: XsdAnyUri,
    pub updated: SystemTime,
}

static TEN_MINUTES: Duration = Duration::from_secs(60 * 10);

impl Info {
    pub fn outdated(&self) -> bool {
        self.updated + TEN_MINUTES < SystemTime::now()
    }
}

impl Instance {
    pub fn outdated(&self) -> bool {
        self.updated + TEN_MINUTES < SystemTime::now()
    }
}

impl Contact {
    pub fn outdated(&self) -> bool {
        self.updated + TEN_MINUTES < SystemTime::now()
    }
}
