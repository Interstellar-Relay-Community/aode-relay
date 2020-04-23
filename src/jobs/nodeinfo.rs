use crate::jobs::JobState;
use activitystreams::primitives::XsdAnyUri;
use anyhow::Error;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct QueryNodeinfo {
    listener: XsdAnyUri,
}

impl QueryNodeinfo {
    pub fn new(listener: XsdAnyUri) -> Self {
        QueryNodeinfo { listener }
    }

    async fn perform(mut self, state: JobState) -> Result<(), Error> {
        let listener = self.listener.clone();

        if !state.node_cache.is_nodeinfo_outdated(&listener).await {
            return Ok(());
        }

        let url = self.listener.as_url_mut();
        url.set_fragment(None);
        url.set_query(None);
        url.set_path(".well-known/nodeinfo");

        let well_known = state
            .requests
            .fetch::<WellKnown>(self.listener.as_str())
            .await?;

        let href = if let Some(link) = well_known.links.into_iter().find(|l| l.rel.is_supported()) {
            link.href
        } else {
            return Ok(());
        };

        let nodeinfo = state.requests.fetch::<Nodeinfo>(&href).await?;

        state
            .node_cache
            .set_info(
                &listener,
                nodeinfo.software.name,
                nodeinfo.software.version,
                nodeinfo.open_registrations,
            )
            .await?;
        Ok(())
    }
}

impl ActixJob for QueryNodeinfo {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

    const NAME: &'static str = "relay::jobs::QueryNodeinfo";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(self.perform(state))
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Nodeinfo {
    #[allow(dead_code)]
    version: SupportedVersion,

    software: Software,
    open_registrations: bool,
}

#[derive(serde::Deserialize)]
struct Software {
    name: String,
    version: String,
}

#[derive(serde::Deserialize)]
struct WellKnown {
    links: Vec<Link>,
}

#[derive(serde::Deserialize)]
struct Link {
    rel: MaybeSupported<SupportedNodeinfo>,

    href: String,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum MaybeSupported<T> {
    Supported(T),
    Unsupported(String),
}

impl<T> MaybeSupported<T> {
    fn is_supported(&self) -> bool {
        match self {
            MaybeSupported::Supported(_) => true,
            _ => false,
        }
    }
}

struct SupportedVersion(String);
struct SupportedNodeinfo(String);

static SUPPORTED_VERSIONS: &'static str = "2.";
static SUPPORTED_NODEINFO: &'static str = "http://nodeinfo.diaspora.software/ns/schema/2.";

struct SupportedVersionVisitor;
struct SupportedNodeinfoVisitor;

impl<'de> serde::de::Visitor<'de> for SupportedVersionVisitor {
    type Value = SupportedVersion;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "a string starting with '{}'", SUPPORTED_VERSIONS)
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if s.starts_with(SUPPORTED_VERSIONS) {
            Ok(SupportedVersion(s.to_owned()))
        } else {
            Err(serde::de::Error::custom("Invalid nodeinfo version"))
        }
    }
}

impl<'de> serde::de::Visitor<'de> for SupportedNodeinfoVisitor {
    type Value = SupportedNodeinfo;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "a string starting with '{}'", SUPPORTED_NODEINFO)
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if s.starts_with(SUPPORTED_NODEINFO) {
            Ok(SupportedNodeinfo(s.to_owned()))
        } else {
            Err(serde::de::Error::custom("Invalid nodeinfo version"))
        }
    }
}

impl<'de> serde::de::Deserialize<'de> for SupportedVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(SupportedVersionVisitor)
    }
}

impl<'de> serde::de::Deserialize<'de> for SupportedNodeinfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_str(SupportedNodeinfoVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::{Nodeinfo, WellKnown};

    const BANANA_DOG: &'static str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://banana.dog/nodeinfo/2.0"},{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.1","href":"https://banana.dog/nodeinfo/2.1"}]}"#;
    const ASONIX_DOG: &'static str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://asonix.dog/nodeinfo/2.0"}]}"#;
    const RELAY_ASONIX_DOG: &'static str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://relay.asonix.dog/nodeinfo/2.0.json"}]}"#;

    const BANANA_DOG_NODEINFO: &'static str = r#"{"version":"2.1","software":{"name":"corgidon","version":"3.1.3+corgi","repository":"https://github.com/msdos621/corgidon"},"protocols":["activitypub"],"usage":{"users":{"total":203,"activeMonth":115,"activeHalfyear":224},"localPosts":28856},"openRegistrations":true,"metadata":{"nodeName":"Banana.dog","nodeDescription":"\u003c/p\u003e\r\n\u003cp\u003e\r\nOfficially endorsed by \u003ca href=\"https://mastodon.social/@Gargron/100059130444127703\"\u003e@Gargron\u003c/a\u003e as a joke instance (along with \u003ca href=\"https://freedom.horse/about\"\u003efreedom.horse\u003c/a\u003e).  Things that make banana.dog unique as an instance.\r\n\u003c/p\u003e\r\n\u003cul\u003e\r\n\u003cli\u003eFederates with TOR servers\u003c/li\u003e\r\n\u003cli\u003eStays up to date, often running newest mastodon code\u003c/li\u003e\r\n\u003cli\u003eUnique color scheme\u003c/li\u003e\r\n\u003cli\u003eA thorough set of rules\u003c/li\u003e\r\n\u003cli\u003eA BananaDogInc company.  Visit our other sites sites including \u003ca href=\"https://betamax.video\"\u003ebetaMax.video\u003c/a\u003e, \u003ca href=\"https://psychicdebugging.com\"\u003epsychicdebugging\u003c/a\u003e and \u003ca href=\"https://somebody.once.told.me.the.world.is.gonnaroll.me/\"\u003egonnaroll\u003c/a\u003e\u003c/li\u003e\r\n\u003c/ul\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eWho we are looking for:\u003c/em\u003e\r\nThis instance only allows senior toot engineers. If you have at least 10+ years of mastodon experience please apply here (https://banana.dog). We are looking for rockstar ninja rocket scientists and we offer unlimited PTO as well as a fully stocked snack bar (with soylent). We are a lean, agile, remote friendly mastodon startup that pays in the bottom 25% for senior tooters. All new members get equity via an innovative ICO call BananaCoin.\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eThe interview process\u003c/em\u003e\r\nTo join we have a take home exam that involves you writing several hundred toots that we can use to screen you.  We will then throw these away during your interview so that we can do a technical screening where we use a whiteboard to evaluate your ability to re-toot memes and shitpost in front of a panel.  This panel will be composed of senior tooters who are all 30 year old cis white males (coincidence).\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eHere are the reasons you may want to join:\u003c/em\u003e\r\nWe are an agile tooting startup (a tootup). That means for every senior tooter we have a designer, a UX person, a product manager, project manager and scrum master. We meet for 15min every day and plan twice a week in a 3 hour meeting but it’s cool because you get lunch and have to attend. Our tooters love it, I would know if they didn’t since we all have standing desks in an open office layouts d can hear everything!\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003ca href=\"https://www.patreon.com/bePatron?u=178864\" data-patreon-widget-type=\"become-patron-button\"\u003eSupport our sites on Patreon\u003c/a\u003e\r\n\u003c/p\u003e\r\n\u003cp\u003e","nodeTerms":"","siteContactEmail":"corgi@banana.dog","domainCount":5841,"features":["mastodon_api","mastodon_api_streaming"],"invitesEnabled":true,"federation":{"rejectMedia":[],"rejectReports":[],"silence":[],"suspend":[]}},"services":{"outbound":[],"inbound":[]}}"#;
    const ASONIX_DOG_NODEINFO: &'static str = r#"{"version":"2.0","software":{"name":"mastodon","version":"3.1.3-asonix-changes"},"protocols":["activitypub"],"usage":{"users":{"total":19,"activeMonth":5,"activeHalfyear":5},"localPosts":43036},"openRegistrations":false}"#;
    const RELAY_ASONIX_DOG_NODEINFO: &'static str = r#"{"version":"2.0","software":{"name":"aoderelay","version":"v0.1.0-master"},"protocols":["activitypub"],"services":{"inbound":[],"outbound":[]},"openRegistrations":false,"usage":{"users":{"total":1,"activeHalfyear":1,"activeMonth":1},"localPosts":0,"localComments":0},"metadata":{"peers":[],"blocks":[]}}"#;

    #[test]
    fn banana_dog() {
        is_supported(BANANA_DOG);
        de::<Nodeinfo>(BANANA_DOG_NODEINFO);
    }

    #[test]
    fn asonix_dog() {
        is_supported(ASONIX_DOG);
        de::<Nodeinfo>(ASONIX_DOG_NODEINFO);
    }

    #[test]
    fn relay_asonix_dog() {
        is_supported(RELAY_ASONIX_DOG);
        de::<Nodeinfo>(RELAY_ASONIX_DOG_NODEINFO);
    }

    fn is_supported(s: &str) {
        de::<WellKnown>(s)
            .links
            .into_iter()
            .find(|l| l.rel.is_supported())
            .unwrap();
    }

    fn de<T>(s: &str) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        serde_json::from_str(s).unwrap()
    }
}
