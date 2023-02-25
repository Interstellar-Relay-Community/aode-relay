use crate::{
    error::{Error, ErrorKind},
    jobs::{Boolish, JobState, QueryContact},
};
use activitystreams::{iri, iri_string::types::IriString, primitives::OneOrMany};
use background_jobs::ActixJob;
use std::{fmt::Debug, future::Future, pin::Pin};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct QueryNodeinfo {
    actor_id: IriString,
}

impl Debug for QueryNodeinfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryNodeinfo")
            .field("actor_id", &self.actor_id.to_string())
            .finish()
    }
}

impl QueryNodeinfo {
    pub(crate) fn new(actor_id: IriString) -> Self {
        QueryNodeinfo { actor_id }
    }

    #[tracing::instrument(name = "Query node info", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        if !state
            .node_cache
            .is_nodeinfo_outdated(self.actor_id.clone())
            .await
        {
            return Ok(());
        }

        let authority = self
            .actor_id
            .authority_str()
            .ok_or(ErrorKind::MissingDomain)?;
        let scheme = self.actor_id.scheme_str();
        let well_known_uri = iri!(format!("{scheme}://{authority}/.well-known/nodeinfo"));

        let well_known = match state
            .requests
            .fetch_json::<WellKnown>(&well_known_uri)
            .await
        {
            Ok(well_known) => well_known,
            Err(e) if e.is_breaker() => {
                tracing::debug!("Not retrying due to failed breaker");
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        let href = if let Some(link) = well_known.links.into_iter().find(|l| l.rel.is_supported()) {
            iri!(&link.href)
        } else {
            return Ok(());
        };

        let nodeinfo = match state.requests.fetch_json::<Nodeinfo>(&href).await {
            Ok(nodeinfo) => nodeinfo,
            Err(e) if e.is_breaker() => {
                tracing::debug!("Not retrying due to failed breaker");
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        state
            .node_cache
            .set_info(
                self.actor_id.clone(),
                nodeinfo.software.name,
                nodeinfo.software.version,
                *nodeinfo.open_registrations,
            )
            .await?;

        if let Some(accounts) = nodeinfo
            .metadata
            .and_then(|meta| meta.into_iter().next().and_then(|meta| meta.staff_accounts))
        {
            if let Some(contact_id) = accounts.get(0) {
                state
                    .job_server
                    .queue(QueryContact::new(self.actor_id, contact_id.clone()))
                    .await?;
            }
        }

        Ok(())
    }
}

impl ActixJob for QueryNodeinfo {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::QueryNodeinfo";
    const QUEUE: &'static str = "maintenance";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Nodeinfo {
    #[allow(dead_code)]
    version: SupportedVersion,

    software: Software,
    open_registrations: Boolish,
    metadata: Option<OneOrMany<Metadata>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    staff_accounts: Option<Vec<IriString>>,
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
        matches!(self, MaybeSupported::Supported(_))
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
        write!(f, "a string starting with '{SUPPORTED_VERSIONS}'")
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
        write!(f, "a string starting with '{SUPPORTED_NODEINFO}'")
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
    use activitystreams::iri_string::types::IriString;

    const BANANA_DOG: &str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://banana.dog/nodeinfo/2.0"},{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.1","href":"https://banana.dog/nodeinfo/2.1"}]}"#;
    const ASONIX_DOG: &str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://asonix.dog/nodeinfo/2.0"}]}"#;
    const ASONIX_DOG_4: &str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://masto.asonix.dog/nodeinfo/2.0"}]}"#;
    const RELAY_ASONIX_DOG: &str = r#"{"links":[{"rel":"http://nodeinfo.diaspora.software/ns/schema/2.0","href":"https://relay.asonix.dog/nodeinfo/2.0.json"}]}"#;
    const HYNET: &str = r#"{"links":[{"href":"https://soc.hyena.network/nodeinfo/2.0.json","rel":"http://nodeinfo.diaspora.software/ns/schema/2.0"},{"href":"https://soc.hyena.network/nodeinfo/2.1.json","rel":"http://nodeinfo.diaspora.software/ns/schema/2.1"}]}"#;
    const NEW_HYNET: &str = r#"{"links":[{"href":"https://soc.hyena.network/nodeinfo/2.0.json","rel":"http://nodeinfo.diaspora.software/ns/schema/2.0"},{"href":"https://soc.hyena.network/nodeinfo/2.1.json","rel":"http://nodeinfo.diaspora.software/ns/schema/2.1"}]}"#;

    const BANANA_DOG_NODEINFO: &str = r#"{"version":"2.1","software":{"name":"corgidon","version":"3.1.3+corgi","repository":"https://github.com/msdos621/corgidon"},"protocols":["activitypub"],"usage":{"users":{"total":203,"activeMonth":115,"activeHalfyear":224},"localPosts":28856},"openRegistrations":true,"metadata":{"nodeName":"Banana.dog","nodeDescription":"\u003c/p\u003e\r\n\u003cp\u003e\r\nOfficially endorsed by \u003ca href=\"https://mastodon.social/@Gargron/100059130444127703\"\u003e@Gargron\u003c/a\u003e as a joke instance (along with \u003ca href=\"https://freedom.horse/about\"\u003efreedom.horse\u003c/a\u003e).  Things that make banana.dog unique as an instance.\r\n\u003c/p\u003e\r\n\u003cul\u003e\r\n\u003cli\u003eFederates with TOR servers\u003c/li\u003e\r\n\u003cli\u003eStays up to date, often running newest mastodon code\u003c/li\u003e\r\n\u003cli\u003eUnique color scheme\u003c/li\u003e\r\n\u003cli\u003eA thorough set of rules\u003c/li\u003e\r\n\u003cli\u003eA BananaDogInc company.  Visit our other sites sites including \u003ca href=\"https://betamax.video\"\u003ebetaMax.video\u003c/a\u003e, \u003ca href=\"https://psychicdebugging.com\"\u003epsychicdebugging\u003c/a\u003e and \u003ca href=\"https://somebody.once.told.me.the.world.is.gonnaroll.me/\"\u003egonnaroll\u003c/a\u003e\u003c/li\u003e\r\n\u003c/ul\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eWho we are looking for:\u003c/em\u003e\r\nThis instance only allows senior toot engineers. If you have at least 10+ years of mastodon experience please apply here (https://banana.dog). We are looking for rockstar ninja rocket scientists and we offer unlimited PTO as well as a fully stocked snack bar (with soylent). We are a lean, agile, remote friendly mastodon startup that pays in the bottom 25% for senior tooters. All new members get equity via an innovative ICO call BananaCoin.\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eThe interview process\u003c/em\u003e\r\nTo join we have a take home exam that involves you writing several hundred toots that we can use to screen you.  We will then throw these away during your interview so that we can do a technical screening where we use a whiteboard to evaluate your ability to re-toot memes and shitpost in front of a panel.  This panel will be composed of senior tooters who are all 30 year old cis white males (coincidence).\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003cem\u003eHere are the reasons you may want to join:\u003c/em\u003e\r\nWe are an agile tooting startup (a tootup). That means for every senior tooter we have a designer, a UX person, a product manager, project manager and scrum master. We meet for 15min every day and plan twice a week in a 3 hour meeting but it’s cool because you get lunch and have to attend. Our tooters love it, I would know if they didn’t since we all have standing desks in an open office layouts d can hear everything!\r\n\u003c/p\u003e\r\n\u003cp\u003e\r\n\u003ca href=\"https://www.patreon.com/bePatron?u=178864\" data-patreon-widget-type=\"become-patron-button\"\u003eSupport our sites on Patreon\u003c/a\u003e\r\n\u003c/p\u003e\r\n\u003cp\u003e","nodeTerms":"","siteContactEmail":"corgi@banana.dog","domainCount":5841,"features":["mastodon_api","mastodon_api_streaming"],"invitesEnabled":true,"federation":{"rejectMedia":[],"rejectReports":[],"silence":[],"suspend":[]}},"services":{"outbound":[],"inbound":[]}}"#;
    const ASONIX_DOG_NODEINFO: &str = r#"{"version":"2.0","software":{"name":"mastodon","version":"3.1.3-asonix-changes"},"protocols":["activitypub"],"usage":{"users":{"total":19,"activeMonth":5,"activeHalfyear":5},"localPosts":43036},"openRegistrations":false}"#;
    const ASONIX_DOG_4_NODEINFO: &str = r#"{"version":"2.0","software":{"name":"mastodon","version":"4.0.0rc2-asonix-changes"},"protocols":["activitypub"],"services":{"outbound":[],"inbound":[]},"usage":{"users":{"total":7,"activeMonth":4,"activeHalfyear":7},"localPosts":12359},"openRegistrations":false,"metadata":[]}"#;
    const RELAY_ASONIX_DOG_NODEINFO: &str = r#"{"version":"2.0","software":{"name":"aoderelay","version":"v0.1.0-master"},"protocols":["activitypub"],"services":{"inbound":[],"outbound":[]},"openRegistrations":false,"usage":{"users":{"total":1,"activeHalfyear":1,"activeMonth":1},"localPosts":0,"localComments":0},"metadata":{"peers":[],"blocks":[]}}"#;

    const HYNET_NODEINFO: &str = r#"{"metadata":{"accountActivationRequired":true,"features":["pleroma_api","mastodon_api","mastodon_api_streaming","polls","pleroma_explicit_addressing","shareable_emoji_packs","multifetch","pleroma:api/v1/notifications:include_types_filter","media_proxy","chat","relay","safe_dm_mentions","pleroma_emoji_reactions","pleroma_chat_messages"],"federation":{"enabled":true,"exclusions":false,"mrf_policies":["SimplePolicy","EnsureRePrepended"],"mrf_simple":{"accept":[],"avatar_removal":[],"banner_removal":[],"federated_timeline_removal":["botsin.space","humblr.social","switter.at","kinkyelephant.com","mstdn.foxfam.club","dajiaweibo.com"],"followers_only":[],"media_nsfw":["mstdn.jp","wxw.moe","knzk.me","anime.website","pl.nudie.social","neckbeard.xyz","baraag.net","pawoo.net","vipgirlfriend.xxx","humblr.social","switter.at","kinkyelephant.com","sinblr.com","kinky.business","rubber.social"],"media_removal":[],"reject":["gab.com","search.fedi.app","kiwifarms.cc","pawoo.net","2hu.club","gameliberty.club","loli.estate","shitasstits.life","social.homunyan.com","club.super-niche.club","vampire.estate","weeaboo.space","wxw.moe","youkai.town","kowai.youkai.town","preteengirls.biz","vipgirlfriend.xxx","social.myfreecams.com","pleroma.rareome.ga","ligma.pro","nnia.space","dickkickextremist.xyz","freespeechextremist.com","m.gretaoto.ca","7td.org","pl.smuglo.li","pleroma.hatthieves.es","jojo.singleuser.club","anime.website","rage.lol","shitposter.club"],"reject_deletes":[],"report_removal":[]},"quarantined_instances":["freespeechextremist.com","spinster.xyz"]},"fieldsLimits":{"maxFields":10,"maxRemoteFields":20,"nameLength":512,"valueLength":2048},"invitesEnabled":true,"mailerEnabled":true,"nodeDescription":"All the cackling for your hyaenid needs.","nodeName":"HyNET Social","pollLimits":{"max_expiration":31536000,"max_option_chars":200,"max_options":20,"min_expiration":0},"postFormats":["text/plain","text/html","text/markdown","text/bbcode"],"private":false,"restrictedNicknames":[".well-known","~","about","activities","api","auth","check_password","dev","friend-requests","inbox","internal","main","media","nodeinfo","notice","oauth","objects","ostatus_subscribe","pleroma","proxy","push","registration","relay","settings","status","tag","user-search","user_exists","users","web","verify_credentials","update_credentials","relationships","search","confirmation_resend","mfa"],"skipThreadContainment":true,"staffAccounts":["https://soc.hyena.network/users/HyNET","https://soc.hyena.network/users/mel"],"suggestions":{"enabled":false},"uploadLimits":{"avatar":2000000,"background":4000000,"banner":4000000,"general":10000000}},"openRegistrations":true,"protocols":["activitypub"],"services":{"inbound":[],"outbound":[]},"software":{"name":"pleroma","version":"2.2.50-724-gf917285b-develop+HyNET-prod"},"usage":{"localPosts":3444,"users":{"total":19}},"version":"2.0"}"#;
    const NEW_HYNET_NODEINFO: &str = r#"{"metadata":{"accountActivationRequired":true,"features":["pleroma_api","mastodon_api","mastodon_api_streaming","polls","v2_suggestions","pleroma_explicit_addressing","shareable_emoji_packs","multifetch","pleroma:api/v1/notifications:include_types_filter","chat","shout","relay","safe_dm_mentions","pleroma_emoji_reactions","pleroma_chat_messages","exposable_reactions","profile_directory","custom_emoji_reactions"],"federation":{"enabled":true,"exclusions":false,"mrf_hashtag":{"federated_timeline_removal":[],"reject":[],"sensitive":["nsfw"]},"mrf_policies":["SimplePolicy","EnsureRePrepended","HashtagPolicy"],"mrf_simple":{"accept":[],"avatar_removal":[],"banner_removal":[],"federated_timeline_removal":["botsin.space"],"followers_only":[],"media_nsfw":["mstdn.jp","wxw.moe","knzk.me","vipgirlfriend.xxx","humblr.social","switter.at","kinkyelephant.com","sinblr.com","kinky.business","rubber.social"],"media_removal":[],"reject":["*.10minutepleroma.com","101010.pl","13bells.com","2.distsn.org","2hu.club","2ndamendment.social","434.earth","4chan.icu","4qq.org","7td.org","80percent.social","a.nti.social","aaathats3as.com","accela.online","amala.schwartzwelt.xyz","angrytoday.com","anime.website","antitwitter.moe","antivaxxer.icu","archivefedifor.fun","artalley.social","bae.st","bajax.us","baraag.net","bbs.kawa-kun.com","beefyboys.club","beefyboys.win","bikeshed.party","bitcoinhackers.org","bleepp.com","blovice.bahnhof.cz","brighteon.social","buildthatwallandmakeamericagreatagain.trumpislovetrumpis.life","bungle.online","cawfee.club","censorship.icu","chungus.cc","club.darknight-coffee.org","clubcyberia.co","cock.fish","cock.li","comfyboy.club","contrapointsfan.club","coon.town","counter.social","cum.salon","d-fens.systems","definitely-not-archivefedifor.fun","degenerates.fail","desuposter.club","detroitriotcity.com","developer.gab.com","dogwhipping.day","eientei.org","enigmatic.observer","eveningzoo.club","exited.eu","federation.krowverse.services","fedi.cc","fedi.krowverse.services","fedi.pawlicker.com","fedi.vern.cc","freak.university","freeatlantis.com","freecumextremist.com","freesoftwareextremist.com","freespeech.firedragonstudios.com","freespeech.host","freespeechextremist.com","freevoice.space","freezepeach.xyz","froth.zone","fuckgov.org","gab.ai","gab.polaris-1.work","gab.protohype.net","gabfed.com","gameliberty.club","gearlandia.haus","gitmo.life","glindr.org","glittersluts.xyz","glowers.club","godspeed.moe","gorf.pub","goyim.app","gs.kawa-kun.com","hagra.net","hallsofamenti.io","hayu.sh","hentai.baby","honkwerx.tech","hunk.city","husk.site","iddqd.social","ika.moe","isexychat.space","jaeger.website","justicewarrior.social","kag.social","katiehopkinspolitical.icu","kiwifarms.cc","kiwifarms.is","kiwifarms.net","kohrville.net","koyu.space","kys.moe","lain.com","lain.sh","leafposter.club","lets.saynoto.lgbt","liberdon.com","libertarianism.club","ligma.pro","lolis.world","masochi.st","masthead.social","mastodon.digitalsuccess.dev","mastodon.fidonet.io","mastodon.grin.hu","mastodon.ml","midnightride.rs","milker.cafe","mobile.tmediatech.io","moon.holiday","mstdn.foxfam.club","mstdn.io","mstdn.starnix.network","mulmeyun.church","nazi.social","neckbeard.xyz","neenster.org","neko.ci","netzsphaere.xyz","newjack.city","nicecrew.digital","nnia.space","noagendasocial.com","norrebro.space","oursocialism.today","ovo.sc","pawoo.net","paypig.org","pedo.school","phreedom.tk","pieville.net","pkteerium.xyz","pl.murky.club","pl.spiderden.net","pl.tkammer.de","pl.zombiecats.run","pleroma.nobodyhasthe.biz","pleroma.runfox.tk","pleroma.site","plr.inferencium.net","pmth.us","poa.st","pod.vladtepesblog.com","political.icu","pooper.social","posting.lolicon.rocks","preteengirls.biz","prout.social","qoto.org","rage.lol","rakket.app","raplst.town","rdrama.cc","ryona.agency","s.sneak.berlin","seal.cafe","sealion.club","search.fedi.app","sementerrori.st","shitposter.club","shortstackran.ch","silkhe.art","sleepy.cafe","soc.mahodou.moe","soc.redeyes.site","social.076.ne.jp","social.anoxinon.de","social.chadland.net","social.freetalklive.com","social.getgle.org","social.handholding.io","social.headsca.la","social.imirhil.fr","social.lovingexpressions.net","social.manalejandro.com","social.midwaytrades.com","social.pseudo-whiskey.bar","social.targaryen.house","social.teci.world","societal.co","society.oftrolls.com","socks.pinnoto.org","socnet.supes.com","solagg.com","spinster.xyz","springbo.cc","stereophonic.space","sunshinegardens.org","theautisticinvestors.quest","thechad.zone","theduran.icu","theosis.church","toot.love","toots.alirezahayati.com","traboone.com","truthsocial.co.in","truthsocial.com","tuusin.misono-ya.info","tweety.icu","unbound.social","unsafe.space","varishangout.net","video.nobodyhasthe.biz","voicenews.icu","voluntaryism.club","waifu.social","weeaboo.space","whinge.town","wolfgirl.bar","workers.dev","wurm.host","xiii.ch","xn--p1abe3d.xn--80asehdb","yggdrasil.social","youjo.love"],"reject_deletes":[],"report_removal":[]},"mrf_simple_info":{"federated_timeline_removal":{"botsin.space":{"reason":"A lot of bot content"}},"media_nsfw":{"humblr.social":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"kinky.business":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"kinkyelephant.com":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"knzk.me":{"reason":"Unmarked nsfw media"},"mstdn.jp":{"reason":"Not sure about the media policy"},"rubber.social":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"sinblr.com":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"switter.at":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"vipgirlfriend.xxx":{"reason":"Unmarked nsfw media"},"wxw.moe":{"reason":"Unmarked nsfw media"}}},"quarantined_instances":[],"quarantined_instances_info":{"quarantined_instances":{}}},"fieldsLimits":{"maxFields":10,"maxRemoteFields":20,"nameLength":512,"valueLength":2048},"invitesEnabled":false,"mailerEnabled":true,"nodeDescription":"Akkoma: The cooler fediverse server","nodeName":"HyNET Social","pollLimits":{"max_expiration":31536000,"max_option_chars":200,"max_options":20,"min_expiration":0},"postFormats":["text/plain","text/html","text/markdown","text/bbcode","text/x.misskeymarkdown"],"private":false,"restrictedNicknames":[".well-known","~","about","activities","api","auth","check_password","dev","friend-requests","inbox","internal","main","media","nodeinfo","notice","oauth","objects","ostatus_subscribe","pleroma","proxy","push","registration","relay","settings","status","tag","user-search","user_exists","users","web","verify_credentials","update_credentials","relationships","search","confirmation_resend","mfa"],"skipThreadContainment":true,"staffAccounts":["https://soc.hyena.network/users/mel"],"suggestions":{"enabled":false},"uploadLimits":{"avatar":2000000,"background":4000000,"banner":4000000,"general":16000000}},"openRegistrations":"FALSE","protocols":["activitypub"],"services":{"inbound":[],"outbound":[]},"software":{"name":"akkoma","version":"3.0.0"},"usage":{"localPosts":7,"users":{"total":1}},"version":"2.0"}"#;

    #[test]
    fn hyena_network() {
        is_supported(HYNET);
        let nodeinfo = de::<Nodeinfo>(HYNET_NODEINFO);
        let accounts = nodeinfo
            .metadata
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .staff_accounts
            .unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(
            accounts[0],
            "https://soc.hyena.network/users/HyNET"
                .parse::<IriString>()
                .unwrap()
        );
    }

    #[test]
    fn new_hyena_network() {
        is_supported(NEW_HYNET);
        let nodeinfo = de::<Nodeinfo>(NEW_HYNET_NODEINFO);
        let accounts = nodeinfo
            .metadata
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .staff_accounts
            .unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            accounts[0],
            "https://soc.hyena.network/users/mel"
                .parse::<IriString>()
                .unwrap()
        );
    }

    #[test]
    fn asonix_dog_4() {
        is_supported(ASONIX_DOG_4);
        de::<Nodeinfo>(ASONIX_DOG_4_NODEINFO);
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
