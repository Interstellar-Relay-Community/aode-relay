use crate::{
    db::{Contact, Db, Info, Instance},
    error::{Error, ErrorKind},
};
use activitystreams::{iri, iri_string::types::IriString};
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug)]
pub struct NodeCache {
    db: Db,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Node {
    pub(crate) base: IriString,
    pub(crate) info: Option<Info>,
    pub(crate) instance: Option<Instance>,
    pub(crate) contact: Option<Contact>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("base", &self.base.to_string())
            .field("info", &self.info)
            .field("instance", &self.instance)
            .field("contact", &self.contact)
            .finish()
    }
}

impl NodeCache {
    pub(crate) fn new(db: Db) -> Self {
        NodeCache { db }
    }

    #[tracing::instrument(level = "debug", name = "Get nodes", skip(self))]
    pub(crate) async fn nodes(&self) -> Result<Vec<Node>, Error> {
        let infos = self.db.connected_info().await?;
        let instances = self.db.connected_instance().await?;
        let contacts = self.db.connected_contact().await?;

        let vec = self
            .db
            .connected_ids()
            .await?
            .into_iter()
            .map(move |actor_id| {
                let info = infos.get(&actor_id).cloned();
                let instance = instances.get(&actor_id).cloned();
                let contact = contacts.get(&actor_id).cloned();

                Node::new(actor_id).map(|node| node.info(info).instance(instance).contact(contact))
            })
            .collect::<Result<Vec<Node>, Error>>()?;

        Ok(vec)
    }

    #[tracing::instrument(level = "debug", name = "Is NodeInfo Outdated", skip_all, fields(actor_id = actor_id.to_string().as_str()))]
    pub(crate) async fn is_nodeinfo_outdated(&self, actor_id: IriString) -> bool {
        self.db
            .info(actor_id)
            .await
            .map(|opt| opt.map(|info| info.outdated()).unwrap_or(true))
            .unwrap_or(true)
    }

    #[tracing::instrument(level = "debug", name = "Is Contact Outdated", skip_all, fields(actor_id = actor_id.to_string().as_str()))]
    pub(crate) async fn is_contact_outdated(&self, actor_id: IriString) -> bool {
        self.db
            .contact(actor_id)
            .await
            .map(|opt| opt.map(|contact| contact.outdated()).unwrap_or(true))
            .unwrap_or(true)
    }

    #[tracing::instrument(level = "debug", name = "Is Instance Outdated", skip_all, fields(actor_id = actor_id.to_string().as_str()))]
    pub(crate) async fn is_instance_outdated(&self, actor_id: IriString) -> bool {
        self.db
            .instance(actor_id)
            .await
            .map(|opt| opt.map(|instance| instance.outdated()).unwrap_or(true))
            .unwrap_or(true)
    }

    #[tracing::instrument(level = "debug", name = "Save node info", skip_all, fields(actor_id = actor_id.to_string().as_str(), software, version, reg))]
    pub(crate) async fn set_info(
        &self,
        actor_id: IriString,
        software: String,
        version: String,
        reg: bool,
    ) -> Result<(), Error> {
        self.db
            .save_info(
                actor_id,
                Info {
                    software,
                    version,
                    reg,
                    updated: SystemTime::now(),
                },
            )
            .await
    }

    #[tracing::instrument(
        level = "debug",
        name = "Save instance info",
        skip_all,
        fields(
            actor_id = actor_id.to_string().as_str(),
            title,
            description,
            version,
            reg,
            requires_approval
        )
    )]
    pub(crate) async fn set_instance(
        &self,
        actor_id: IriString,
        title: String,
        description: String,
        version: String,
        reg: bool,
        requires_approval: bool,
    ) -> Result<(), Error> {
        self.db
            .save_instance(
                actor_id,
                Instance {
                    title,
                    description,
                    version,
                    reg,
                    requires_approval,
                    updated: SystemTime::now(),
                },
            )
            .await
    }

    #[tracing::instrument(
        level = "debug",
        name = "Save contact info",
        skip_all,
        fields(
            actor_id = actor_id.to_string().as_str(),
            username,
            display_name,
            url = url.to_string().as_str(),
            avatar = avatar.to_string().as_str()
        )
    )]
    pub(crate) async fn set_contact(
        &self,
        actor_id: IriString,
        username: String,
        display_name: String,
        url: IriString,
        avatar: IriString,
    ) -> Result<(), Error> {
        self.db
            .save_contact(
                actor_id,
                Contact {
                    username,
                    display_name,
                    url,
                    avatar,
                    updated: SystemTime::now(),
                },
            )
            .await
    }
}

impl Node {
    fn new(url: IriString) -> Result<Self, Error> {
        let authority = url.authority_str().ok_or(ErrorKind::MissingDomain)?;
        let scheme = url.scheme_str();

        let base = iri!(format!("{scheme}://{authority}"));

        Ok(Node {
            base,
            info: None,
            instance: None,
            contact: None,
        })
    }

    fn info(mut self, info: Option<Info>) -> Self {
        self.info = info;
        self
    }

    fn instance(mut self, instance: Option<Instance>) -> Self {
        self.instance = instance;
        self
    }

    fn contact(mut self, contact: Option<Contact>) -> Self {
        self.contact = contact;
        self
    }
}

static TEN_MINUTES: Duration = Duration::from_secs(60 * 10);

impl Info {
    pub(crate) fn outdated(&self) -> bool {
        self.updated + TEN_MINUTES < SystemTime::now()
    }
}

impl Instance {
    pub(crate) fn outdated(&self) -> bool {
        self.updated + TEN_MINUTES < SystemTime::now()
    }
}

impl Contact {
    pub(crate) fn outdated(&self) -> bool {
        self.updated + TEN_MINUTES < SystemTime::now()
    }
}
