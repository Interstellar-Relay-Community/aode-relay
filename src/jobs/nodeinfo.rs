use crate::jobs::{JobState, QueryContact};
use activitystreams::url::Url;
use anyhow::Error;
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct QueryNodeinfo {
    actor_id: Url,
}

impl QueryNodeinfo {
    pub(crate) fn new(actor_id: Url) -> Self {
        QueryNodeinfo { actor_id }
    }

    async fn perform(self, state: JobState) -> Result<(), Error> {
        if !state
            .node_cache
            .is_nodeinfo_outdated(self.actor_id.clone())
            .await
        {
            return Ok(());
        }

        let mut well_known_uri = self.actor_id.clone();
        well_known_uri.set_fragment(None);
        well_known_uri.set_query(None);
        well_known_uri.set_path(".well-known/nodeinfo");

        let well_known = state
            .requests
            .fetch_json::<WellKnown>(well_known_uri.as_str())
            .await?;

        let href = if let Some(link) = well_known.links.into_iter().find(|l| l.rel.is_supported()) {
            link.href
        } else {
            return Ok(());
        };

        let nodeinfo = state.requests.fetch_json::<Nodeinfo>(&href).await?;

        state
            .node_cache
            .set_info(
                self.actor_id.clone(),
                nodeinfo.software.name,
                nodeinfo.software.version,
                nodeinfo.open_registrations,
            )
            .await?;

        if let Some(accounts) = nodeinfo.metadata.and_then(|meta| meta.staff_accounts) {
            if let Some(contact_id) = accounts.get(0) {
                state
                    .job_server
                    .queue(QueryContact::new(self.actor_id, contact_id.clone()))?;
            }
        }

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
    metadata: Option<Metadata>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    staff_accounts: Option<Vec<Url>>,
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

static SUPPORTED_VERSIONS: &str = "2.";
static SUPPORTED_NODEINFO: &str = "http://nodeinfo.diaspora.software/ns/schema/2.";

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
    use activitystreams::url::Url;

    const BANANA_DOG: &'static str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://banana.dog/nodeinfo/2.0"},{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.1","href":"https://banana.dog/nodeinfo/2.1"}]}"#;
    const ASONIX_DOG: &'static str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://asonix.dog/nodeinfo/2.0"}]}"#;
    const RELAY_ASONIX_DOG: &'static str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://relay.asonix.dog/nodeinfo/2.0.json"}]}"#;
    const HYNET: &'static str = r#"{"links":[{"href":"https://soc.hyena.network/nodeinfo/2.0.json","rel":"http://nodeinfo.diaspora.software/ns/schema/2.0"},{"href":"https://soc.hyena.network/nodeinfo/2.1.json","rel":"http://nodeinfo.diaspora.software/ns/schema/2.1"}]}"#;

    const BANANA_DOG_NODEINFO: &'static str = r#"{"version":"2.1","software":{"name":"corgidon","version":"3.1.3+corgi","repository":"https://github.com/msdos621/corgidon"},"protocols":["activitypub"],"usage":{"users":{"total":203,"activeMonth":115,"activeHalfyear":224},"localPosts":28856},"openRegistrations":true,"metadata":{"nodeName":"Banana.dog","nodeDescription":"\u003c/p\u003e\r\n\u003cp\u003e\r\nOfficially endorsed by \u003ca href=\"https://mastodon.social/@Gargron/100059130444127703\"\u003e@Gargron\u003c/a\u003e as a joke instance (along with \u003ca href=\"https://freedom.horse/about\"\u003efreedom.horse\u003c/a\u003e).  Things that make banana.dog unique as an instance.\r\n\u003c/p\u003e\r\n\u003cul\u003e\r\n\u003cli\u003eFederates with TOR servers\u003c/li\u003e\r\n\u003cli\u003eStays up to date, often running newest mastodon code\u003c/li\u003e\r\n\u003cli\u003eUnique color scheme\u003c/li\u003e\r\n\u003cli\u003eA thorough set of rules\u003c/li\u003e\r\n\u003cli\u003eA BananaDogInc company.  Visit our other sites sites including \u003ca href=\"https://betamax.video\"\u003ebetaMax.video\u003c/a\u003e, \u003ca href=\"https://psychicdebugging.com\"\u003epsychicdebugging\u003c/a\u003e and \u003ca href=\"https://somebody.once.told.me.the.world.is.gonnaroll.me/\"\u003egonnaroll\u003c/a\u003e\u003c/li\u003e\r\n\u003c/ul\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eWho we are looking for:\u003c/em\u003e\r\nThis instance only allows senior toot engineers. If you have at least 10+ years of mastodon experience please apply here (https://banana.dog). We are looking for rockstar ninja rocket scientists and we offer unlimited PTO as well as a fully stocked snack bar (with soylent). We are a lean, agile, remote friendly mastodon startup that pays in the bottom 25% for senior tooters. All new members get equity via an innovative ICO call BananaCoin.\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eThe interview process\u003c/em\u003e\r\nTo join we have a take home exam that involves you writing several hundred toots that we can use to screen you.  We will then throw these away during your interview so that we can do a technical screening where we use a whiteboard to evaluate your ability to re-toot memes and shitpost in front of a panel.  This panel will be composed of senior tooters who are all 30 year old cis white males (coincidence).\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eHere are the reasons you may want to join:\u003c/em\u003e\r\nWe are an agile tooting startup (a tootup). That means for every senior tooter we have a designer, a UX person, a product manager, project manager and scrum master. We meet for 15min every day and plan twice a week in a 3 hour meeting but it’s cool because you get lunch and have to attend. Our tooters love it, I would know if they didn’t since we all have standing desks in an open office layouts d can hear everything!\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003ca href=\"https://www.patreon.com/bePatron?u=178864\" data-patreon-widget-type=\"become-patron-button\"\u003eSupport our sites on Patreon\u003c/a\u003e\r\n\u003c/p\u003e\r\n\u003cp\u003e","nodeTerms":"","siteContactEmail":"corgi@banana.dog","domainCount":5841,"features":["mastodon_api","mastodon_api_streaming"],"invitesEnabled":true,"federation":{"rejectMedia":[],"rejectReports":[],"silence":[],"suspend":[]}},"services":{"outbound":[],"inbound":[]}}"#;
    const ASONIX_DOG_NODEINFO: &'static str = r#"{"version":"2.0","software":{"name":"mastodon","version":"3.1.3-asonix-changes"},"protocols":["activitypub"],"usage":{"users":{"total":19,"activeMonth":5,"activeHalfyear":5},"localPosts":43036},"openRegistrations":false}"#;
    const RELAY_ASONIX_DOG_NODEINFO: &'static str = r#"{"version":"2.0","software":{"name":"aoderelay","version":"v0.1.0-master"},"protocols":["activitypub"],"services":{"inbound":[],"outbound":[]},"openRegistrations":false,"usage":{"users":{"total":1,"activeHalfyear":1,"activeMonth":1},"localPosts":0,"localComments":0},"metadata":{"peers":[],"blocks":[]}}"#;

    const HYNET_NODEINFO: &'static str = r#"{"metadata":{"accountActivationRequired":true,"features":["pleroma_api","mastodon_api","mastodon_api_streaming","polls","pleroma_explicit_addressing","shareable_emoji_packs","multifetch","pleroma:api/v1/notifications:include_types_filter","media_proxy","chat","relay","safe_dm_mentions","pleroma_emoji_reactions","pleroma_chat_messages"],"federation":{"enabled":true,"exclusions":false,"mrf_policies":["SimplePolicy","EnsureRePrepended"],"mrf_simple":{"accept":[],"avatar_removal":[],"banner_removal":[],"federated_timeline_removal":["botsin.space","humblr.social","switter.at","kinkyelephant.com","mstdn.foxfam.club","dajiaweibo.com"],"followers_only":[],"media_nsfw":["mstdn.jp","wxw.moe","knzk.me","anime.website","pl.nudie.social","neckbeard.xyz","baraag.net","pawoo.net","vipgirlfriend.xxx","humblr.social","switter.at","kinkyelephant.com","sinblr.com","kinky.business","rubber.social"],"media_removal":[],"reject":["gab.com","search.fedi.app","kiwifarms.cc","pawoo.net","2hu.club","gameliberty.club","loli.estate","shitasstits.life","social.homunyan.com","club.super-niche.club","vampire.estate","weeaboo.space","wxw.moe","youkai.town","kowai.youkai.town","preteengirls.biz","vipgirlfriend.xxx","social.myfreecams.com","pleroma.rareome.ga","ligma.pro","nnia.space","dickkickextremist.xyz","freespeechextremist.com","m.gretaoto.ca","7td.org","pl.smuglo.li","pleroma.hatthieves.es","jojo.singleuser.club","anime.website","rage.lol","shitposter.club"],"reject_deletes":[],"report_removal":[]},"quarantined_instances":["freespeechextremist.com","spinster.xyz"]},"fieldsLimits":{"maxFields":10,"maxRemoteFields":20,"nameLength":512,"valueLength":2048},"invitesEnabled":true,"mailerEnabled":true,"nodeDescription":"All the cackling for your hyaenid needs.","nodeName":"HyNET Social","pollLimits":{"max_expiration":31536000,"max_option_chars":200,"max_options":20,"min_expiration":0},"postFormats":["text/plain","text/html","text/markdown","text/bbcode"],"private":false,"restrictedNicknames":[".well-known","~","about","activities","api","auth","check_password","dev","friend-requests","inbox","internal","main","media","nodeinfo","notice","oauth","objects","ostatus_subscribe","pleroma","proxy","push","registration","relay","settings","status","tag","user-search","user_exists","users","web","verify_credentials","update_credentials","relationships","search","confirmation_resend","mfa"],"skipThreadContainment":true,"staffAccounts":["https://soc.hyena.network/users/HyNET","https://soc.hyena.network/users/mel"],"suggestions":{"enabled":false},"uploadLimits":{"avatar":2000000,"background":4000000,"banner":4000000,"general":10000000}},"openRegistrations":true,"protocols":["activitypub"],"services":{"inbound":[],"outbound":[]},"software":{"name":"pleroma","version":"2.2.50-724-gf917285b-develop+HyNET-prod"},"usage":{"localPosts":3444,"users":{"total":19}},"version":"2.0"}"#;

    #[test]
    fn hyena_network() {
        is_supported(HYNET);
        let nodeinfo = de::<Nodeinfo>(HYNET_NODEINFO);
        let accounts = nodeinfo.metadata.unwrap().staff_accounts.unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(
            accounts[0],
            "https://soc.hyena.network/users/HyNET"
                .parse::<Url>()
                .unwrap()
        );
    }

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
