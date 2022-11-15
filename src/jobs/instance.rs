use crate::{
    config::UrlKind,
    error::{Error, ErrorKind},
    jobs::{cache_media::CacheMedia, JobState},
};
use activitystreams::{iri, iri_string::types::IriString};
use background_jobs::ActixJob;
use std::{future::Future, pin::Pin};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct QueryInstance {
    actor_id: IriString,
}

impl std::fmt::Debug for QueryInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryInstance")
            .field("actor_id", &self.actor_id.to_string())
            .finish()
    }
}

impl QueryInstance {
    pub(crate) fn new(actor_id: IriString) -> Self {
        QueryInstance { actor_id }
    }

    #[tracing::instrument(name = "Query instance", skip(state))]
    async fn perform(self, state: JobState) -> Result<(), Error> {
        let contact_outdated = state
            .node_cache
            .is_contact_outdated(self.actor_id.clone())
            .await;
        let instance_outdated = state
            .node_cache
            .is_instance_outdated(self.actor_id.clone())
            .await;

        if !(contact_outdated || instance_outdated) {
            return Ok(());
        }

        let authority = self
            .actor_id
            .authority_str()
            .ok_or(ErrorKind::MissingDomain)?;
        let scheme = self.actor_id.scheme_str();
        let instance_uri = iri!(format!("{}://{}/api/v1/instance", scheme, authority));

        let instance = state
            .requests
            .fetch_json::<Instance>(instance_uri.as_str())
            .await?;

        let description = instance.short_description.unwrap_or(instance.description);

        if let Some(contact) = instance.contact {
            let uuid = if let Some(uuid) = state.media.get_uuid(contact.avatar.clone()).await? {
                uuid
            } else {
                state.media.store_url(contact.avatar).await?
            };

            let avatar = state.config.generate_url(UrlKind::Media(uuid));

            state.job_server.queue(CacheMedia::new(uuid)).await?;

            state
                .node_cache
                .set_contact(
                    self.actor_id.clone(),
                    contact.username,
                    contact.display_name,
                    contact.url,
                    avatar,
                )
                .await?;
        }

        let description = ammonia::clean(&description);

        state
            .node_cache
            .set_instance(
                self.actor_id,
                instance.title,
                description,
                instance.version,
                *instance.registrations,
                instance.approval_required,
            )
            .await?;

        Ok(())
    }
}

impl ActixJob for QueryInstance {
    type State = JobState;
    type Future = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>>>>;

    const NAME: &'static str = "relay::jobs::QueryInstance";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}

fn default_approval() -> bool {
    false
}

struct Boolish {
    inner: bool,
}

impl std::ops::Deref for Boolish {
    type Target = bool;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'de> serde::Deserialize<'de> for Boolish {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum BoolThing {
            Bool(bool),
            String(String),
        }

        let thing: BoolThing = serde::Deserialize::deserialize(deserializer)?;

        match thing {
            BoolThing::Bool(inner) => Ok(Boolish { inner }),
            BoolThing::String(s) if s.to_lowercase() == "false" => Ok(Boolish { inner: false }),
            BoolThing::String(_) => Ok(Boolish { inner: true }),
        }
    }
}

#[derive(serde::Deserialize)]
struct Instance {
    title: String,
    short_description: Option<String>,
    description: String,
    version: String,
    registrations: Boolish,

    #[serde(default = "default_approval")]
    approval_required: bool,

    #[serde(rename = "contact_account")]
    contact: Option<Contact>,
}

#[derive(serde::Deserialize)]
struct Contact {
    username: String,
    display_name: String,
    url: IriString,
    avatar: IriString,
}

#[cfg(test)]
mod tests {
    use super::{Boolish, Instance};

    const ASONIX_INSTANCE: &'static str = r#"{"uri":"masto.asonix.dog","title":"asonix.dog","short_description":"The asonix of furry mastodon. For me and a few friends. DM me somewhere if u want an account lol","description":"A mastodon server that's only for me and nobody else sorry","email":"asonix@asonix.dog","version":"4.0.0rc2-asonix-changes","urls":{"streaming_api":"wss://masto.asonix.dog"},"stats":{"user_count":7,"status_count":12328,"domain_count":5146},"thumbnail":"https://masto.asonix.dog/system/site_uploads/files/000/000/002/@1x/32f51462a2b2bf2d.png","languages":["dog"],"registrations":false,"approval_required":false,"invites_enabled":false,"configuration":{"accounts":{"max_featured_tags":10},"statuses":{"max_characters":500,"max_media_attachments":4,"characters_reserved_per_url":23},"media_attachments":{"supported_mime_types":["image/jpeg","image/png","image/gif","image/heic","image/heif","image/webp","image/avif","video/webm","video/mp4","video/quicktime","video/ogg","audio/wave","audio/wav","audio/x-wav","audio/x-pn-wave","audio/vnd.wave","audio/ogg","audio/vorbis","audio/mpeg","audio/mp3","audio/webm","audio/flac","audio/aac","audio/m4a","audio/x-m4a","audio/mp4","audio/3gpp","video/x-ms-asf"],"image_size_limit":10485760,"image_matrix_limit":16777216,"video_size_limit":41943040,"video_frame_rate_limit":60,"video_matrix_limit":2304000},"polls":{"max_options":4,"max_characters_per_option":50,"min_expiration":300,"max_expiration":2629746}},"contact_account":{"id":"1","username":"asonix","acct":"asonix","display_name":"Liom on Mane :antiverified:","locked":true,"bot":false,"discoverable":true,"group":false,"created_at":"2021-02-09T00:00:00.000Z","note":"\u003cp\u003e26, local liom, friend, rust (lang) stan, bi \u003c/p\u003e\u003cp\u003eicon by \u003cspan class=\"h-card\"\u003e\u003ca href=\"https://furaffinity.net/user/lalupine\" target=\"blank\" rel=\"noopener noreferrer\" class=\"u-url mention\"\u003e@\u003cspan\u003elalupine@furaffinity.net\u003c/span\u003e\u003c/a\u003e\u003c/span\u003e\u003cbr /\u003eheader by \u003cspan class=\"h-card\"\u003e\u003ca href=\"https://furaffinity.net/user/tronixx\" target=\"blank\" rel=\"noopener noreferrer\" class=\"u-url mention\"\u003e@\u003cspan\u003etronixx@furaffinity.net\u003c/span\u003e\u003c/a\u003e\u003c/span\u003e\u003c/p\u003e\u003cp\u003eTestimonials:\u003c/p\u003e\u003cp\u003eStand: LIONS\u003cbr /\u003eStand User: AODE\u003cbr /\u003e- Keris (not on here)\u003c/p\u003e","url":"https://masto.asonix.dog/@asonix","avatar":"https://masto.asonix.dog/system/accounts/avatars/000/000/001/original/00852df0e6fee7e0.png","avatar_static":"https://masto.asonix.dog/system/accounts/avatars/000/000/001/original/00852df0e6fee7e0.png","header":"https://masto.asonix.dog/system/accounts/headers/000/000/001/original/8122ce3e5a745385.png","header_static":"https://masto.asonix.dog/system/accounts/headers/000/000/001/original/8122ce3e5a745385.png","followers_count":237,"following_count":474,"statuses_count":8798,"last_status_at":"2022-11-08","noindex":true,"emojis":[{"shortcode":"antiverified","url":"https://masto.asonix.dog/system/custom_emojis/images/000/030/053/original/bb0bc2e395b9a127.png","static_url":"https://masto.asonix.dog/system/custom_emojis/images/000/030/053/static/bb0bc2e395b9a127.png","visible_in_picker":true}],"fields":[{"name":"pronouns","value":"he/they","verified_at":null},{"name":"software","value":"bad","verified_at":null},{"name":"gitea","value":"\u003ca href=\"https://git.asonix.dog\" target=\"_blank\" rel=\"nofollow noopener noreferrer me\"\u003e\u003cspan class=\"invisible\"\u003ehttps://\u003c/span\u003e\u003cspan class=\"\"\u003egit.asonix.dog\u003c/span\u003e\u003cspan class=\"invisible\"\u003e\u003c/span\u003e\u003c/a\u003e","verified_at":null},{"name":"join my","value":"relay","verified_at":null}]},"rules":[]}"#;
    const HYNET_INSTANCE: &'static str = r#"{"approval_required":false,"avatar_upload_limit":2000000,"background_image":"https://soc.hyena.network/images/city.jpg","background_upload_limit":4000000,"banner_upload_limit":4000000,"description":"Akkoma: The cooler fediverse server","description_limit":5000,"email":"me@hyena.network","languages":["en"],"max_toot_chars":"5000","pleroma":{"metadata":{"account_activation_required":true,"features":["pleroma_api","mastodon_api","mastodon_api_streaming","polls","v2_suggestions","pleroma_explicit_addressing","shareable_emoji_packs","multifetch","pleroma:api/v1/notifications:include_types_filter","chat","shout","relay","safe_dm_mentions","pleroma_emoji_reactions","pleroma_chat_messages","exposable_reactions","profile_directory","custom_emoji_reactions"],"federation":{"enabled":true,"exclusions":false,"mrf_hashtag":{"federated_timeline_removal":[],"reject":[],"sensitive":["nsfw"]},"mrf_policies":["SimplePolicy","EnsureRePrepended","HashtagPolicy"],"mrf_simple":{"accept":[],"avatar_removal":[],"banner_removal":[],"federated_timeline_removal":["botsin.space"],"followers_only":[],"media_nsfw":["mstdn.jp","wxw.moe","knzk.me","vipgirlfriend.xxx","humblr.social","switter.at","kinkyelephant.com","sinblr.com","kinky.business","rubber.social"],"media_removal":[],"reject":["*.10minutepleroma.com","101010.pl","13bells.com","2.distsn.org","2hu.club","2ndamendment.social","434.earth","4chan.icu","4qq.org","7td.org","80percent.social","a.nti.social","aaathats3as.com","accela.online","amala.schwartzwelt.xyz","angrytoday.com","anime.website","antitwitter.moe","antivaxxer.icu","archivefedifor.fun","artalley.social","bae.st","bajax.us","baraag.net","bbs.kawa-kun.com","beefyboys.club","beefyboys.win","bikeshed.party","bitcoinhackers.org","bleepp.com","blovice.bahnhof.cz","brighteon.social","buildthatwallandmakeamericagreatagain.trumpislovetrumpis.life","bungle.online","cawfee.club","censorship.icu","chungus.cc","club.darknight-coffee.org","clubcyberia.co","cock.fish","cock.li","comfyboy.club","contrapointsfan.club","coon.town","counter.social","cum.salon","d-fens.systems","definitely-not-archivefedifor.fun","degenerates.fail","desuposter.club","detroitriotcity.com","developer.gab.com","dogwhipping.day","eientei.org","enigmatic.observer","eveningzoo.club","exited.eu","federation.krowverse.services","fedi.cc","fedi.krowverse.services","fedi.pawlicker.com","fedi.vern.cc","freak.university","freeatlantis.com","freecumextremist.com","freesoftwareextremist.com","freespeech.firedragonstudios.com","freespeech.host","freespeechextremist.com","freevoice.space","freezepeach.xyz","froth.zone","fuckgov.org","gab.ai","gab.polaris-1.work","gab.protohype.net","gabfed.com","gameliberty.club","gearlandia.haus","gitmo.life","glindr.org","glittersluts.xyz","glowers.club","godspeed.moe","gorf.pub","goyim.app","gs.kawa-kun.com","hagra.net","hallsofamenti.io","hayu.sh","hentai.baby","honkwerx.tech","hunk.city","husk.site","iddqd.social","ika.moe","isexychat.space","jaeger.website","justicewarrior.social","kag.social","katiehopkinspolitical.icu","kiwifarms.cc","kiwifarms.is","kiwifarms.net","kohrville.net","koyu.space","kys.moe","lain.com","lain.sh","leafposter.club","lets.saynoto.lgbt","liberdon.com","libertarianism.club","ligma.pro","lolis.world","masochi.st","masthead.social","mastodon.digitalsuccess.dev","mastodon.fidonet.io","mastodon.grin.hu","mastodon.ml","midnightride.rs","milker.cafe","mobile.tmediatech.io","moon.holiday","mstdn.foxfam.club","mstdn.io","mstdn.starnix.network","mulmeyun.church","nazi.social","neckbeard.xyz","neenster.org","neko.ci","netzsphaere.xyz","newjack.city","nicecrew.digital","nnia.space","noagendasocial.com","norrebro.space","oursocialism.today","ovo.sc","pawoo.net","paypig.org","pedo.school","phreedom.tk","pieville.net","pkteerium.xyz","pl.murky.club","pl.spiderden.net","pl.tkammer.de","pl.zombiecats.run","pleroma.nobodyhasthe.biz","pleroma.runfox.tk","pleroma.site","plr.inferencium.net","pmth.us","poa.st","pod.vladtepesblog.com","political.icu","pooper.social","posting.lolicon.rocks","preteengirls.biz","prout.social","qoto.org","rage.lol","rakket.app","raplst.town","rdrama.cc","ryona.agency","s.sneak.berlin","seal.cafe","sealion.club","search.fedi.app","sementerrori.st","shitposter.club","shortstackran.ch","silkhe.art","sleepy.cafe","soc.mahodou.moe","soc.redeyes.site","social.076.ne.jp","social.anoxinon.de","social.chadland.net","social.freetalklive.com","social.getgle.org","social.handholding.io","social.headsca.la","social.imirhil.fr","social.lovingexpressions.net","social.manalejandro.com","social.midwaytrades.com","social.pseudo-whiskey.bar","social.targaryen.house","social.teci.world","societal.co","society.oftrolls.com","socks.pinnoto.org","socnet.supes.com","solagg.com","spinster.xyz","springbo.cc","stereophonic.space","sunshinegardens.org","theautisticinvestors.quest","thechad.zone","theduran.icu","theosis.church","toot.love","toots.alirezahayati.com","traboone.com","truthsocial.co.in","truthsocial.com","tuusin.misono-ya.info","tweety.icu","unbound.social","unsafe.space","varishangout.net","video.nobodyhasthe.biz","voicenews.icu","voluntaryism.club","waifu.social","weeaboo.space","whinge.town","wolfgirl.bar","workers.dev","wurm.host","xiii.ch","xn--p1abe3d.xn--80asehdb","yggdrasil.social","youjo.love"],"reject_deletes":[],"report_removal":[]},"mrf_simple_info":{"federated_timeline_removal":{"botsin.space":{"reason":"A lot of bot content"}},"media_nsfw":{"humblr.social":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"kinky.business":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"kinkyelephant.com":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"knzk.me":{"reason":"Unmarked nsfw media"},"mstdn.jp":{"reason":"Not sure about the media policy"},"rubber.social":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"sinblr.com":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"switter.at":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"vipgirlfriend.xxx":{"reason":"Unmarked nsfw media"},"wxw.moe":{"reason":"Unmarked nsfw media"}}},"quarantined_instances":[],"quarantined_instances_info":{"quarantined_instances":{}}},"fields_limits":{"max_fields":10,"max_remote_fields":20,"name_length":512,"value_length":2048},"post_formats":["text/plain","text/html","text/markdown","text/bbcode","text/x.misskeymarkdown"],"privileged_staff":false},"stats":{"mau":1},"vapid_public_key":"BMg4q-rT3rkMzc29F7OS5uM6t-Rx4HncMIB1NXrKwNlVRfX-W1kwgOuq5pDy-WhWmOZudaegftjBTCX3-pzdDFc"},"poll_limits":{"max_expiration":31536000,"max_option_chars":200,"max_options":20,"min_expiration":0},"registrations":"FALSE","shout_limit":5000,"stats":{"domain_count":1035,"status_count":7,"user_count":1},"thumbnail":"https://soc.hyena.network/instance/thumbnail.jpeg","title":"HyNET Social","upload_limit":16000000,"uri":"https://soc.hyena.network","urls":{"streaming_api":"wss://soc.hyena.network"},"version":"2.7.2 (compatible; Akkoma 3.0.0)"}"#;

    #[test]
    fn boolish_works() {
        const CASES: &[(&str, bool)] = &[
            ("false", false),
            ("\"false\"", false),
            ("\"FALSE\"", false),
            ("true", true),
            ("\"true\"", true),
            ("\"anything else\"", true),
        ];

        for (case, output) in CASES {
            let b: Boolish = serde_json::from_str(case).unwrap();
            assert_eq!(*b, *output);
        }
    }

    #[test]
    fn deser_masto_instance_with_contact() {
        let inst: Instance = serde_json::from_str(ASONIX_INSTANCE).unwrap();
        let _ = inst.contact.unwrap();
    }

    #[test]
    fn deser_akkoma_instance_no_contact() {
        let inst: Instance = serde_json::from_str(HYNET_INSTANCE).unwrap();
        assert!(inst.contact.is_none());
    }
}
