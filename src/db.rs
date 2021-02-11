use crate::{config::Config, error::MyError};
use activitystreams::url::Url;
use actix_web::web::Bytes;
use rsa::RSAPrivateKey;
use rsa_pem::KeyExt;
use sled::Tree;
use std::{collections::HashMap, sync::Arc, time::SystemTime};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct Db {
    inner: Arc<Inner>,
}

struct Inner {
    actor_id_actor: Tree,
    public_key_id_actor_id: Tree,
    connected_actor_ids: Tree,
    allowed_domains: Tree,
    blocked_domains: Tree,
    settings: Tree,
    media_url_media_id: Tree,
    media_id_media_url: Tree,
    media_id_media_bytes: Tree,
    media_id_media_meta: Tree,
    actor_id_info: Tree,
    actor_id_instance: Tree,
    actor_id_contact: Tree,
    restricted_mode: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Actor {
    pub(crate) id: Url,
    pub(crate) public_key: String,
    pub(crate) public_key_id: Url,
    pub(crate) inbox: Url,
    pub(crate) saved_at: SystemTime,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct MediaMeta {
    pub(crate) media_type: String,
    pub(crate) saved_at: SystemTime,
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

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Contact {
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) url: Url,
    pub(crate) avatar: Url,
    pub(crate) updated: SystemTime,
}

impl Inner {
    fn connected_by_domain(&self, domains: &[String]) -> impl DoubleEndedIterator<Item = Url> {
        let reversed: Vec<_> = domains
            .into_iter()
            .map(|s| domain_key(s.as_str()))
            .collect();

        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(url_from_ivec)
            .filter_map(move |url| {
                let connected_domain = url.domain()?;
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

    fn connected(&self) -> impl DoubleEndedIterator<Item = Url> {
        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(url_from_ivec)
    }

    fn connected_actors<'a>(&'a self) -> impl DoubleEndedIterator<Item = Actor> + 'a {
        self.connected_actor_ids
            .iter()
            .values()
            .filter_map(|res| res.ok())
            .filter_map(move |actor_id| {
                let actor_ivec = self.actor_id_actor.get(actor_id).ok()??;

                serde_json::from_slice::<Actor>(&actor_ivec).ok()
            })
    }

    fn connected_info<'a>(&'a self) -> impl DoubleEndedIterator<Item = (Url, Info)> + 'a {
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

    fn connected_instance<'a>(&'a self) -> impl DoubleEndedIterator<Item = (Url, Instance)> + 'a {
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

    fn connected_contact<'a>(&'a self) -> impl DoubleEndedIterator<Item = (Url, Contact)> + 'a {
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

    fn is_allowed(&self, domain: &str) -> bool {
        let prefix = domain_prefix(domain);
        let reverse_domain = domain_key(domain);

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
    pub(crate) fn build(config: &Config) -> Result<Self, MyError> {
        let db = sled::open(config.sled_path())?;
        Self::build_inner(config.restricted_mode(), db)
    }

    fn build_inner(restricted_mode: bool, db: sled::Db) -> Result<Self, MyError> {
        Ok(Db {
            inner: Arc::new(Inner {
                actor_id_actor: db.open_tree("actor-id-actor")?,
                public_key_id_actor_id: db.open_tree("public-key-id-actor-id")?,
                connected_actor_ids: db.open_tree("connected-actor-ids")?,
                allowed_domains: db.open_tree("allowed-actor-ids")?,
                blocked_domains: db.open_tree("blocked-actor-ids")?,
                settings: db.open_tree("settings")?,
                media_url_media_id: db.open_tree("media-url-media-id")?,
                media_id_media_url: db.open_tree("media-id-media-url")?,
                media_id_media_bytes: db.open_tree("media-id-media-bytes")?,
                media_id_media_meta: db.open_tree("media-id-media-meta")?,
                actor_id_info: db.open_tree("actor-id-info")?,
                actor_id_instance: db.open_tree("actor-id-instance")?,
                actor_id_contact: db.open_tree("actor-id-contact")?,
                restricted_mode,
            }),
        })
    }

    async fn unblock<T>(
        &self,
        f: impl Fn(&Inner) -> Result<T, MyError> + Send + 'static,
    ) -> Result<T, MyError>
    where
        T: Send + 'static,
    {
        let inner = self.inner.clone();

        let t = actix_web::web::block(move || (f)(&inner)).await??;

        Ok(t)
    }

    pub(crate) async fn connected_ids(&self) -> Result<Vec<Url>, MyError> {
        self.unblock(|inner| Ok(inner.connected().collect())).await
    }

    pub(crate) async fn save_info(&self, actor_id: Url, info: Info) -> Result<(), MyError> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&info)?;

            inner
                .actor_id_info
                .insert(actor_id.as_str().as_bytes(), vec)?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn info(&self, actor_id: Url) -> Result<Option<Info>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.actor_id_info.get(actor_id.as_str().as_bytes())? {
                let info = serde_json::from_slice(&ivec)?;
                Ok(Some(info))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn connected_info(&self) -> Result<HashMap<Url, Info>, MyError> {
        self.unblock(|inner| Ok(inner.connected_info().collect()))
            .await
    }

    pub(crate) async fn save_instance(
        &self,
        actor_id: Url,
        instance: Instance,
    ) -> Result<(), MyError> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&instance)?;

            inner
                .actor_id_instance
                .insert(actor_id.as_str().as_bytes(), vec)?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn instance(&self, actor_id: Url) -> Result<Option<Instance>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.actor_id_instance.get(actor_id.as_str().as_bytes())? {
                let instance = serde_json::from_slice(&ivec)?;
                Ok(Some(instance))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn connected_instance(&self) -> Result<HashMap<Url, Instance>, MyError> {
        self.unblock(|inner| Ok(inner.connected_instance().collect()))
            .await
    }

    pub(crate) async fn save_contact(
        &self,
        actor_id: Url,
        contact: Contact,
    ) -> Result<(), MyError> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&contact)?;

            inner
                .actor_id_contact
                .insert(actor_id.as_str().as_bytes(), vec)?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn contact(&self, actor_id: Url) -> Result<Option<Contact>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.actor_id_contact.get(actor_id.as_str().as_bytes())? {
                let contact = serde_json::from_slice(&ivec)?;
                Ok(Some(contact))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn connected_contact(&self) -> Result<HashMap<Url, Contact>, MyError> {
        self.unblock(|inner| Ok(inner.connected_contact().collect()))
            .await
    }

    pub(crate) async fn save_url(&self, url: Url, id: Uuid) -> Result<(), MyError> {
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

    pub(crate) async fn save_bytes(
        &self,
        id: Uuid,
        meta: MediaMeta,
        bytes: Bytes,
    ) -> Result<(), MyError> {
        self.unblock(move |inner| {
            let vec = serde_json::to_vec(&meta)?;

            inner
                .media_id_media_bytes
                .insert(id.as_bytes(), bytes.as_ref())?;
            inner.media_id_media_meta.insert(id.as_bytes(), vec)?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn media_id(&self, url: Url) -> Result<Option<Uuid>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.media_url_media_id.get(url.as_str().as_bytes())? {
                Ok(uuid_from_ivec(ivec))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn media_url(&self, id: Uuid) -> Result<Option<Url>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.media_id_media_url.get(id.as_bytes())? {
                Ok(url_from_ivec(ivec))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn media_bytes(&self, id: Uuid) -> Result<Option<Bytes>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.media_id_media_bytes.get(id.as_bytes())? {
                Ok(Some(Bytes::copy_from_slice(&ivec)))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn media_meta(&self, id: Uuid) -> Result<Option<MediaMeta>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.media_id_media_meta.get(id.as_bytes())? {
                let meta = serde_json::from_slice(&ivec)?;
                Ok(Some(meta))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn blocks(&self) -> Result<Vec<String>, MyError> {
        self.unblock(|inner| Ok(inner.blocks().collect())).await
    }

    pub(crate) async fn inboxes(&self) -> Result<Vec<Url>, MyError> {
        self.unblock(|inner| Ok(inner.connected_actors().map(|actor| actor.inbox).collect()))
            .await
    }

    pub(crate) async fn is_connected(&self, mut id: Url) -> Result<bool, MyError> {
        id.set_path("");
        id.set_query(None);
        id.set_fragment(None);

        self.unblock(move |inner| {
            let connected = inner
                .connected_actor_ids
                .scan_prefix(id.as_str().as_bytes())
                .values()
                .filter_map(|res| res.ok())
                .next()
                .is_some();

            Ok(connected)
        })
        .await
    }

    pub(crate) async fn actor_id_from_public_key_id(
        &self,
        public_key_id: Url,
    ) -> Result<Option<Url>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner
                .public_key_id_actor_id
                .get(public_key_id.as_str().as_bytes())?
            {
                Ok(url_from_ivec(ivec))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn actor(&self, actor_id: Url) -> Result<Option<Actor>, MyError> {
        self.unblock(move |inner| {
            if let Some(ivec) = inner.actor_id_actor.get(actor_id.as_str().as_bytes())? {
                let actor = serde_json::from_slice(&ivec)?;
                Ok(Some(actor))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn save_actor(&self, actor: Actor) -> Result<(), MyError> {
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

    pub(crate) async fn remove_connection(&self, actor_id: Url) -> Result<(), MyError> {
        log::debug!("Removing Connection: {}", actor_id);
        self.unblock(move |inner| {
            inner
                .connected_actor_ids
                .remove(actor_id.as_str().as_bytes())?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn add_connection(&self, actor_id: Url) -> Result<(), MyError> {
        log::debug!("Adding Connection: {}", actor_id);
        self.unblock(move |inner| {
            inner
                .connected_actor_ids
                .insert(actor_id.as_str().as_bytes(), actor_id.as_str().as_bytes())?;

            Ok(())
        })
        .await
    }

    pub(crate) async fn add_blocks(&self, domains: Vec<String>) -> Result<(), MyError> {
        self.unblock(move |inner| {
            for connected in inner.connected_by_domain(&domains) {
                inner
                    .connected_actor_ids
                    .remove(connected.as_str().as_bytes())?;
            }

            for domain in &domains {
                inner
                    .blocked_domains
                    .insert(domain_key(domain), domain.as_bytes())?;
                inner.allowed_domains.remove(domain_key(domain))?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn remove_blocks(&self, domains: Vec<String>) -> Result<(), MyError> {
        self.unblock(move |inner| {
            for domain in &domains {
                inner.blocked_domains.remove(domain_key(domain))?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn add_allows(&self, domains: Vec<String>) -> Result<(), MyError> {
        self.unblock(move |inner| {
            for domain in &domains {
                inner
                    .allowed_domains
                    .insert(domain_key(domain), domain.as_bytes())?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn remove_allows(&self, domains: Vec<String>) -> Result<(), MyError> {
        self.unblock(move |inner| {
            if inner.restricted_mode {
                for connected in inner.connected_by_domain(&domains) {
                    inner
                        .connected_actor_ids
                        .remove(connected.as_str().as_bytes())?;
                }
            }

            for domain in &domains {
                inner.allowed_domains.remove(domain_key(domain))?;
            }

            Ok(())
        })
        .await
    }

    pub(crate) async fn is_allowed(&self, url: Url) -> Result<bool, MyError> {
        self.unblock(move |inner| {
            if let Some(domain) = url.domain() {
                Ok(inner.is_allowed(domain))
            } else {
                Ok(false)
            }
        })
        .await
    }

    pub(crate) async fn private_key(&self) -> Result<Option<RSAPrivateKey>, MyError> {
        self.unblock(|inner| {
            if let Some(ivec) = inner.settings.get("private-key")? {
                let key_str = String::from_utf8_lossy(&ivec);
                let key = RSAPrivateKey::from_pem_pkcs8(&key_str)?;

                Ok(Some(key))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub(crate) async fn update_private_key(
        &self,
        private_key: &RSAPrivateKey,
    ) -> Result<(), MyError> {
        let pem_pkcs8 = private_key.to_pem_pkcs8()?;

        self.unblock(move |inner| {
            inner
                .settings
                .insert("private-key".as_bytes(), pem_pkcs8.as_bytes())?;
            Ok(())
        })
        .await
    }
}

fn domain_key(domain: &str) -> String {
    domain.split('.').rev().collect::<Vec<_>>().join(".") + "."
}

fn domain_prefix(domain: &str) -> String {
    domain
        .split('.')
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .join(".")
        + "."
}

fn url_from_ivec(ivec: sled::IVec) -> Option<Url> {
    String::from_utf8_lossy(&ivec).parse::<Url>().ok()
}

fn uuid_from_ivec(ivec: sled::IVec) -> Option<Uuid> {
    Uuid::from_slice(&ivec).ok()
}

#[cfg(test)]
mod tests {
    use super::Db;
    use activitystreams::url::Url;
    use std::future::Future;

    #[test]
    fn connect_and_verify() {
        run(|db| async move {
            let example_actor: Url = "http://example.com/actor".parse().unwrap();
            let example_sub_actor: Url = "http://example.com/users/fake".parse().unwrap();
            db.add_connection(example_actor.clone()).await.unwrap();
            assert!(db.is_connected(example_sub_actor).await.unwrap());
        })
    }

    #[test]
    fn disconnect_and_verify() {
        run(|db| async move {
            let example_actor: Url = "http://example.com/actor".parse().unwrap();
            let example_sub_actor: Url = "http://example.com/users/fake".parse().unwrap();
            db.add_connection(example_actor.clone()).await.unwrap();
            assert!(db.is_connected(example_sub_actor.clone()).await.unwrap());

            db.remove_connection(example_actor).await.unwrap();
            assert!(!db.is_connected(example_sub_actor).await.unwrap());
        })
    }

    #[test]
    fn connected_actor_in_connected_list() {
        run(|db| async move {
            let example_actor: Url = "http://example.com/actor".parse().unwrap();
            db.add_connection(example_actor.clone()).await.unwrap();

            assert!(db.connected_ids().await.unwrap().contains(&example_actor));
        })
    }

    #[test]
    fn disconnected_actor_not_in_connected_list() {
        run(|db| async move {
            let example_actor: Url = "http://example.com/actor".parse().unwrap();
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
        actix_rt::System::new("test").block_on((f)(db));
    }
}
