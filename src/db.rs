use crate::{
    config::Config,
    error::{Error, ErrorKind},
};
use activitystreams::iri_string::types::IriString;
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
    RsaPrivateKey,
};
use sled::{Batch, Tree};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::SystemTime,
};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(crate) struct Db {
    inner: Arc<Inner>,
}

struct Inner {
    healthz: Tree,
    healthz_counter: Arc<AtomicU64>,
    actor_id_actor: Tree,
    public_key_id_actor_id: Tree,
    connected_actor_ids: Tree,
    allowed_domains: Tree,
    blocked_domains: Tree,
    settings: Tree,
    media_url_media_id: Tree,
    media_id_media_url: Tree,
    actor_id_info: Tree,
    actor_id_instance: Tree,
    actor_id_contact: Tree,
    last_seen: Tree,
    restricted_mode: bool,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("restricted_mode", &self.restricted_mode)
            .finish()
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Actor {
    pub(crate) id: IriString,
    pub(crate) public_key: String,
    pub(crate) public_key_id: IriString,
    pub(crate) inbox: IriString,
    pub(crate) saved_at: SystemTime,
}

impl std::fmt::Debug for Actor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actor")
            .field("id", &self.id.to_string())
            .field("public_key", &self.public_key)
            .field("public_key_id", &self.public_key_id.to_string())
            .field("inbox", &self.inbox.to_string())
            .field("saved_at", &self.saved_at)
            .finish()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Info {
    pub(crate) software: String,
    pub(crate) version: String,
    pub(crate) reg: bool,
    pub(crate) updated: SystemTime,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Instance {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) version: String,
    pub(crate) reg: bool,
    pub(crate) requires_approval: bool,
    pub(crate) updated: SystemTime,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct Contact {
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) url: IriString,
    pub(crate) avatar: IriString,
    pub(crate) updated: SystemTime,
}

impl std::fmt::Debug for Contact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Info")
            .field("username", &self.username)
            .field("display_name", &self.display_name)
            .field("url", &self.url.to_string())
            .field("avatar", &self.avatar.to_string())
            .field("updated", &self.updated)
            .finish()
    }
}

impl Inner {
    fn connected_by_domain(
        &self,
        domains: &[String],
    ) -> impl DoubleEndedIterator<Item = IriString> {
        let reversed: Vec<_> = domains.iter().map(|s| domain_key(s.as_str())).collect();

        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(url_from_ivec)
            .filter_map(move |url| {
                let connected_domain = url.authority_str()?;
                let connected_rdnn = domain_key(connected_domain);

                for rdnn in &reversed {
                    if connected_rdnn.starts_with(rdnn) {
                        return Some(url);
                    }
                }

                None
            })
    }

    fn blocks(&self) -> impl DoubleEndedIterator<Item = String> {
        self.blocked_domains
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .map(|s| String::from_utf8_lossy(&s).to_string())
    }

    fn allowed(&self) -> impl DoubleEndedIterator<Item = String> {
        self.allowed_domains
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .map(|s| String::from_utf8_lossy(&s).to_string())
    }

    fn connected(&self) -> impl DoubleEndedIterator<Item = IriString> {
        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(url_from_ivec)
    }

    fn connected_actors(&self) -> impl DoubleEndedIterator<Item = Actor> + '_ {
        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(move |actor_id| {
                let actor_ivec = self.actor_id_actor.get(actor_id).ok()??;

                serde_json::from_slice::<Actor>(&actor_ivec).ok()
            })
    }

    fn connected_info(&self) -> impl DoubleEndedIterator<Item = (IriString, Info)> + '_ {
        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(move |actor_id_ivec| {
                let actor_id = url_from_ivec(actor_id_ivec.clone())?;
                let ivec = self.actor_id_info.get(actor_id_ivec).ok()??;
                let info = serde_json::from_slice(&ivec).ok()?;

                Some((actor_id, info))
            })
    }

    fn connected_instance(&self) -> impl DoubleEndedIterator<Item = (IriString, Instance)> + '_ {
        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(move |actor_id_ivec| {
                let actor_id = url_from_ivec(actor_id_ivec.clone())?;
                let ivec = self.actor_id_instance.get(actor_id_ivec).ok()??;
                let instance = serde_json::from_slice(&ivec).ok()?;

                Some((actor_id, instance))
            })
    }

    fn connected_contact(&self) -> impl DoubleEndedIterator<Item = (IriString, Contact)> + '_ {
        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(move |actor_id_ivec| {
                let actor_id = url_from_ivec(actor_id_ivec.clone())?;
                let ivec = self.actor_id_contact.get(actor_id_ivec).ok()??;
                let contact = serde_json::from_slice(&ivec).ok()?;

                Some((actor_id, contact))
            })
    }

    fn is_allowed(&self, authority: &str) -> bool {
        let prefix = domain_prefix(authority);
        let reverse_domain = domain_key(authority);

        if self.restricted_mode {
            self.allowed_domains
                .scan_prefix(prefix)
                .keys()
                .filter_map(|res| res.ok())
                .any(|rdnn| {
                    let rdnn_string = String::from_utf8_lossy(&rdnn);
                    reverse_domain.starts_with(rdnn_string.as_ref())
                })
        } else {
            !self
                .blocked_domains
                .scan_prefix(prefix)
                .keys()
                .filter_map(|res| res.ok())
                .any(|rdnn| reverse_domain.starts_with(String::from_utf8_lossy(&rdnn).as_ref()))
        }
    }
}

impl Db {
    pub(crate) fn build(config: &Config) -> Result<Self, Error> {
        let db = sled::open(config.sled_path())?;
        Self::build_inner(config.restricted_mode(), db)
    }

    fn build_inner(restricted_mode: bool, db: sled::Db) -> Result<Self, Error> {
        Ok(Db {
            inner: Arc::new(Inner {
                healthz: db.open_tree("healthz")?,
                healthz_counter: Arc::new(AtomicU64::new(0)),
                actor_id_actor: db.open_tree("actor-id-actor")?,
                public_key_id_actor_id: db.open_tree("public-key-id-actor-id")?,
                connected_actor_ids: db.open_tree("connected-actor-ids")?,
                allowed_domains: db.open_tree("allowed-actor-ids")?,
                blocked_domains: db.open_tree("blocked-actor-ids")?,
                settings: db.open_tree("settings")?,
                media_url_media_id: db.open_tree("media-url-media-id")?,
                media_id_media_url: db.open_tree("media-id-media-url")?,
                actor_id_info: db.open_tree("actor-id-info")?,
                actor_id_instance: db.open_tree("actor-id-instance")?,
                actor_id_contact: db.open_tree("actor-id-contact")?,
                last_seen: db.open_tree("last-seen")?,
                restricted_mode,
            }),
        })
    }

    async fn unblock<T>(
        &self,
        f: impl FnOnce(&Inner) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error>
    where
        T: Send + 'static,
    {
        let inner = self.inner.clone();

        let t = actix_web::web::block(move || (f)(&inner)).await??;

        Ok(t)
    }

    pub(crate) async fn check_health(&self) -> Result<(), Error> {
        let next = self.inner.healthz_counter.fetch_add(1, Ordering::Relaxed);
        self.unblock(move |inner| {
            inner
                .healthz
                .insert("healthz", &next.to_be_bytes()[..])
                .map_err(Error::from)
        })
        .await?;
        self.inner.healthz.flush_async().await?;
        self.unblock(move |inner| inner.healthz.get("healthz").map_err(Error::from))
            .await?;
        Ok(())
    }

    pub(crate) async fn mark_last_seen(
        &self,
        nodes: HashMap<String, OffsetDateTime>,
    ) -> Result<(), Error> {
        let mut batch = Batch::default();

        for (domain, datetime) in nodes {
            let datetime_string = serde_json::to_vec(&datetime)?;

            batch.insert(domain.as_bytes(), datetime_string);
        }

        self.unblock(move |inner| inner.last_seen.apply_batch(batch).map_err(Error::from))
            .await
    }

    pub(crate) async fn last_seen(
        &self,
    ) -> Result<BTreeMap<String, Option<OffsetDateTime>>, Error> {
        self.unblock(|inner| {
            let mut map = BTreeMap::new();

            for iri in inner.connected() {
                let Some(authority_str) = iri.authority_str() else {
                    continue;
                };

                if let Some(datetime) = inner.last_seen.get(authority_str)? {
                    map.insert(
                        authority_str.to_string(),
                        Some(serde_json::from_slice(&datetime)?),
                    );
                } else {
                    map.insert(authority_str.to_string(), None);
                }
            }

            Ok(map)
        })
        .await
    }

    pub(crate) async fn connected_ids(&self) -> Result<Vec<IriString>, Error> {
        self.unblock(|inner| Ok(inner.connected().collect())).await
    }

    pub(crate) async fn save_info(&self, actor_id: IriString, info: Info) -> Result<(), Error> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&info)?;

            inner
                .actor_id_info
                .insert(actor_id.as_str().as_bytes(), vec)?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn info(&self, actor_id: IriString) -> Result<Option<Info>, Error> {
        self.unblock(move |inner| {
            inner
                .actor_id_info
                .get(actor_id.as_str().as_bytes())?
                .map(|ivec| serde_json::from_slice(&ivec))
                .transpose()
                .map_err(Error::from)
        })
        .await
    }

    pub(crate) async fn connected_info(&self) -> Result<HashMap<IriString, Info>, Error> {
        self.unblock(|inner| Ok(inner.connected_info().collect()))
            .await
    }

    pub(crate) async fn save_instance(
        &self,
        actor_id: IriString,
        instance: Instance,
    ) -> Result<(), Error> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&instance)?;

            inner
                .actor_id_instance
                .insert(actor_id.as_str().as_bytes(), vec)?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn instance(&self, actor_id: IriString) -> Result<Option<Instance>, Error> {
        self.unblock(move |inner| {
            inner
                .actor_id_instance
                .get(actor_id.as_str().as_bytes())?
                .map(|ivec| serde_json::from_slice(&ivec))
                .transpose()
                .map_err(Error::from)
        })
        .await
    }

    pub(crate) async fn connected_instance(&self) -> Result<HashMap<IriString, Instance>, Error> {
        self.unblock(|inner| Ok(inner.connected_instance().collect()))
            .await
    }

    pub(crate) async fn save_contact(
        &self,
        actor_id: IriString,
        contact: Contact,
    ) -> Result<(), Error> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&contact)?;

            inner
                .actor_id_contact
                .insert(actor_id.as_str().as_bytes(), vec)?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn contact(&self, actor_id: IriString) -> Result<Option<Contact>, Error> {
        self.unblock(move |inner| {
            inner
                .actor_id_contact
                .get(actor_id.as_str().as_bytes())?
                .map(|ivec| serde_json::from_slice(&ivec))
                .transpose()
                .map_err(Error::from)
        })
        .await
    }

    pub(crate) async fn connected_contact(&self) -> Result<HashMap<IriString, Contact>, Error> {
        self.unblock(|inner| Ok(inner.connected_contact().collect()))
            .await
    }

    pub(crate) async fn save_url(&self, url: IriString, id: Uuid) -> Result<(), Error> {
        self.unblock(move |inner| {
            inner
                .media_id_media_url
                .insert(id.as_bytes(), url.as_str().as_bytes())?;
            inner
                .media_url_media_id
                .insert(url.as_str().as_bytes(), id.as_bytes())?;
            Ok(())
        })
        .await
    }

    pub(crate) async fn media_id(&self, url: IriString) -> Result<Option<Uuid>, Error> {
        self.unblock(move |inner| {
            Ok(inner
                .media_url_media_id
                .get(url.as_str().as_bytes())?
                .and_then(uuid_from_ivec))
        })
        .await
    }

    pub(crate) async fn media_url(&self, id: Uuid) -> Result<Option<IriString>, Error> {
        self.unblock(move |inner| {
            Ok(inner
                .media_id_media_url
                .get(id.as_bytes())?
                .and_then(url_from_ivec))
        })
        .await
    }

    pub(crate) async fn blocks(&self) -> Result<Vec<String>, Error> {
        self.unblock(|inner| Ok(inner.blocks().collect())).await
    }

    pub(crate) async fn allows(&self) -> Result<Vec<String>, Error> {
        self.unblock(|inner| Ok(inner.allowed().collect())).await
    }

    pub(crate) async fn inboxes(&self) -> Result<Vec<IriString>, Error> {
        self.unblock(|inner| Ok(inner.connected_actors().map(|actor| actor.inbox).collect()))
            .await
    }

    pub(crate) async fn is_connected(&self, base_id: IriString) -> Result<bool, Error> {
        let scheme = base_id.scheme_str();
        let authority = base_id.authority_str().ok_or(ErrorKind::MissingDomain)?;
        let prefix = format!("{scheme}://{authority}");

        self.unblock(move |inner| {
            let connected = inner
                .connected_actor_ids
                .scan_prefix(prefix.as_bytes())
                .values()
                .any(|res| res.is_ok());

            Ok(connected)
        })
        .await
    }

    pub(crate) async fn actor_id_from_public_key_id(
        &self,
        public_key_id: IriString,
    ) -> Result<Option<IriString>, Error> {
        self.unblock(move |inner| {
            Ok(inner
                .public_key_id_actor_id
                .get(public_key_id.as_str().as_bytes())?
                .and_then(url_from_ivec))
        })
        .await
    }

    pub(crate) async fn actor(&self, actor_id: IriString) -> Result<Option<Actor>, Error> {
        self.unblock(move |inner| {
            inner
                .actor_id_actor
                .get(actor_id.as_str().as_bytes())?
                .map(|ivec| serde_json::from_slice(&ivec))
                .transpose()
                .map_err(Error::from)
        })
        .await
    }

    pub(crate) async fn save_actor(&self, actor: Actor) -> Result<(), Error> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&actor)?;

            inner.public_key_id_actor_id.insert(
                actor.public_key_id.as_str().as_bytes(),
                actor.id.as_str().as_bytes(),
            )?;
            inner
                .actor_id_actor
                .insert(actor.id.as_str().as_bytes(), vec)?;
            Ok(())
        })
        .await
    }

    pub(crate) async fn remove_connection(&self, actor_id: IriString) -> Result<(), Error> {
        tracing::debug!("Removing Connection: {actor_id}");
        self.unblock(move |inner| {
            inner
                .connected_actor_ids
                .remove(actor_id.as_str().as_bytes())?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn add_connection(&self, actor_id: IriString) -> Result<(), Error> {
        tracing::debug!("Adding Connection: {actor_id}");
        self.unblock(move |inner| {
            inner
                .connected_actor_ids
                .insert(actor_id.as_str().as_bytes(), actor_id.as_str().as_bytes())?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn add_blocks(&self, domains: Vec<String>) -> Result<(), Error> {
        self.unblock(move |inner| {
            for connected in inner.connected_by_domain(&domains) {
                inner
                    .connected_actor_ids
                    .remove(connected.as_str().as_bytes())?;
            }

            for authority in &domains {
                inner
                    .blocked_domains
                    .insert(domain_key(authority), authority.as_bytes())?;
                inner.allowed_domains.remove(domain_key(authority))?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn remove_blocks(&self, domains: Vec<String>) -> Result<(), Error> {
        self.unblock(move |inner| {
            for authority in &domains {
                inner.blocked_domains.remove(domain_key(authority))?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn add_allows(&self, domains: Vec<String>) -> Result<(), Error> {
        self.unblock(move |inner| {
            for authority in &domains {
                inner
                    .allowed_domains
                    .insert(domain_key(authority), authority.as_bytes())?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn remove_allows(&self, domains: Vec<String>) -> Result<(), Error> {
        self.unblock(move |inner| {
            if inner.restricted_mode {
                for connected in inner.connected_by_domain(&domains) {
                    inner
                        .connected_actor_ids
                        .remove(connected.as_str().as_bytes())?;
                }
            }

            for authority in &domains {
                inner.allowed_domains.remove(domain_key(authority))?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn is_allowed(&self, url: IriString) -> Result<bool, Error> {
        self.unblock(move |inner| {
            if let Some(authority) = url.authority_str() {
                Ok(inner.is_allowed(authority))
            } else {
                Ok(false)
            }
        })
        .await
    }

    pub(crate) async fn private_key(&self) -> Result<Option<RsaPrivateKey>, Error> {
        self.unblock(|inner| {
            if let Some(ivec) = inner.settings.get("private-key")? {
                let key_str = String::from_utf8_lossy(&ivec);
                let key = RsaPrivateKey::from_pkcs8_pem(&key_str)?;

                Ok(Some(key))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn update_private_key(
        &self,
        private_key: &RsaPrivateKey,
    ) -> Result<(), Error> {
        let pem_pkcs8 = private_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::default())?;

        self.unblock(move |inner| {
            inner
                .settings
                .insert("private-key".as_bytes(), pem_pkcs8.as_bytes())?;
            Ok(())
        })
        .await
    }
}

fn domain_key(authority: &str) -> String {
    authority.split('.').rev().collect::<Vec<_>>().join(".") + "."
}

fn domain_prefix(authority: &str) -> String {
    authority
        .split('.')
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .join(".")
        + "."
}

fn url_from_ivec(ivec: sled::IVec) -> Option<IriString> {
    String::from_utf8_lossy(&ivec).parse::<IriString>().ok()
}

fn uuid_from_ivec(ivec: sled::IVec) -> Option<Uuid> {
    Uuid::from_slice(&ivec).ok()
}

#[cfg(test)]
mod tests {
    use super::Db;
    use activitystreams::iri_string::types::IriString;
    use std::future::Future;

    #[test]
    fn connect_and_verify() {
        run(|db| async move {
            let example_actor: IriString = "http://example.com/actor".parse().unwrap();
            let example_sub_actor: IriString = "http://example.com/users/fake".parse().unwrap();
            db.add_connection(example_actor.clone()).await.unwrap();
            assert!(db.is_connected(example_sub_actor).await.unwrap());
        })
    }

    #[test]
    fn disconnect_and_verify() {
        run(|db| async move {
            let example_actor: IriString = "http://example.com/actor".parse().unwrap();
            let example_sub_actor: IriString = "http://example.com/users/fake".parse().unwrap();
            db.add_connection(example_actor.clone()).await.unwrap();
            assert!(db.is_connected(example_sub_actor.clone()).await.unwrap());

            db.remove_connection(example_actor).await.unwrap();
            assert!(!db.is_connected(example_sub_actor).await.unwrap());
        })
    }

    #[test]
    fn connected_actor_in_connected_list() {
        run(|db| async move {
            let example_actor: IriString = "http://example.com/actor".parse().unwrap();
            db.add_connection(example_actor.clone()).await.unwrap();

            assert!(db.connected_ids().await.unwrap().contains(&example_actor));
        })
    }

    #[test]
    fn disconnected_actor_not_in_connected_list() {
        run(|db| async move {
            let example_actor: IriString = "http://example.com/actor".parse().unwrap();
            db.add_connection(example_actor.clone()).await.unwrap();
            db.remove_connection(example_actor.clone()).await.unwrap();

            assert!(!db.connected_ids().await.unwrap().contains(&example_actor));
        })
    }

    fn run<F, Fut>(f: F)
    where
        F: Fn(Db) -> Fut,
        Fut: Future<Output = ()> + 'static,
    {
        let db =
            Db::build_inner(true, sled::Config::new().temporary(true).open().unwrap()).unwrap();
        actix_rt::System::new().block_on((f)(db));
    }
}
