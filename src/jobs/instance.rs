use crate::{
    config::UrlKind,
    error::{Error, ErrorKind},
    jobs::{Boolish, JobState},
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

enum InstanceApiType {
    Mastodon,
    Misskey,
}

impl QueryInstance {
    pub(crate) fn new(actor_id: IriString) -> Self {
        QueryInstance { actor_id }
    }

    async fn get_instance(
        instance_type: InstanceApiType,
        state: &JobState,
        scheme: &str,
        authority: &str,
    ) -> Result<Instance, Error> {
        match instance_type {
            InstanceApiType::Mastodon => {
                let mastodon_instance_uri = iri!(format!("{scheme}://{authority}/api/v1/instance"));
                state
                    .requests
                    .fetch_json::<Instance>(&mastodon_instance_uri)
                    .await
            }
            InstanceApiType::Misskey => {
                let msky_meta_uri = iri!(format!("{scheme}://{authority}/api/meta"));
                state
                    .requests
                    .fetch_json_msky::<MisskeyMeta>(&msky_meta_uri)
                    .await
                    .map(|res| res.into())
            }
        }
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

        // Attempt all endpoint.
        let instance_futures = [
            Self::get_instance(InstanceApiType::Mastodon, &state, scheme, authority),
            Self::get_instance(InstanceApiType::Misskey, &state, scheme, authority),
        ];

        let mut instance_result: Option<Instance> = None;
        for instance_future in instance_futures {
            match instance_future.await {
                Ok(instance) => {
                    instance_result = Some(instance);
                    break;
                }
                Err(e) if e.is_breaker() => {
                    tracing::debug!("Not retrying due to failed breaker");
                    return Ok(());
                }
                Err(e) if e.is_not_found() => {
                    tracing::debug!("Server doesn't implement instance endpoint");
                }
                Err(e) if e.is_malformed_json() => {
                    tracing::debug!("Server doesn't returned proper json");
                }
                Err(e) => return Err(e),
            }
        }

        let instance = match instance_result {
            Some(instance) => instance,
            None => {
                tracing::debug!("Server doesn't implement all instance endpoint");
                return Ok(());
            }
        };

        let description = instance.short_description.unwrap_or(instance.description);

        if let Some(contact) = instance.contact {
            let uuid = if let Some(uuid) = state.media.get_uuid(contact.avatar.clone()).await? {
                uuid
            } else {
                state.media.store_url(contact.avatar).await?
            };

            let avatar = state.config.generate_url(UrlKind::Media(uuid));

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
    const QUEUE: &'static str = "maintenance";

    fn run(self, state: Self::State) -> Self::Future {
        Box::pin(async move { self.perform(state).await.map_err(Into::into) })
    }
}

fn default_approval() -> bool {
    false
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

#[derive(serde::Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
struct MisskeyMeta {
    name: Option<String>,
    description: Option<String>,
    version: String,
    maintainer_name: Option<String>,
    // Yes, I know, this is instance URL... but we does not have any choice.
    uri: IriString,
    // Use instance icon as a profile picture...
    icon_url: Option<IriString>,
    features: MisskeyFeatures,
}

#[derive(serde::Deserialize)]
struct MisskeyFeatures {
    registration: Boolish, // Corresponding to Mastodon registration
}

impl From<MisskeyMeta> for Instance {
    fn from(meta: MisskeyMeta) -> Self {
        let contact = match (meta.maintainer_name, meta.icon_url) {
            (Some(maintainer), Some(icon)) => Some(Contact {
                username: maintainer.clone(),
                display_name: maintainer,
                url: meta.uri,
                avatar: icon,
            }),
            (_, _) => None,
        };

        // Transform it into Mastodon Instance object
        Instance {
            title: meta.name.unwrap_or_else(|| "".to_owned()),
            short_description: None,
            description: meta.description.unwrap_or_else(|| "".to_owned()),
            version: meta.version,
            registrations: meta.features.registration,
            approval_required: false,
            contact,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Instance;
    use super::MisskeyMeta;

    const ASONIX_INSTANCE: &str = r#"{"uri":"masto.asonix.dog","title":"asonix.dog","short_description":"The asonix of furry mastodon. For me and a few friends. DM me somewhere if u want an account lol","description":"A mastodon server that's only for me and nobody else sorry","email":"asonix@asonix.dog","version":"4.0.0rc2-asonix-changes","urls":{"streaming_api":"wss://masto.asonix.dog"},"stats":{"user_count":7,"status_count":12328,"domain_count":5146},"thumbnail":"https://masto.asonix.dog/system/site_uploads/files/000/000/002/@1x/32f51462a2b2bf2d.png","languages":["dog"],"registrations":false,"approval_required":false,"invites_enabled":false,"configuration":{"accounts":{"max_featured_tags":10},"statuses":{"max_characters":500,"max_media_attachments":4,"characters_reserved_per_url":23},"media_attachments":{"supported_mime_types":["image/jpeg","image/png","image/gif","image/heic","image/heif","image/webp","image/avif","video/webm","video/mp4","video/quicktime","video/ogg","audio/wave","audio/wav","audio/x-wav","audio/x-pn-wave","audio/vnd.wave","audio/ogg","audio/vorbis","audio/mpeg","audio/mp3","audio/webm","audio/flac","audio/aac","audio/m4a","audio/x-m4a","audio/mp4","audio/3gpp","video/x-ms-asf"],"image_size_limit":10485760,"image_matrix_limit":16777216,"video_size_limit":41943040,"video_frame_rate_limit":60,"video_matrix_limit":2304000},"polls":{"max_options":4,"max_characters_per_option":50,"min_expiration":300,"max_expiration":2629746}},"contact_account":{"id":"1","username":"asonix","acct":"asonix","display_name":"Liom on Mane :antiverified:","locked":true,"bot":false,"discoverable":true,"group":false,"created_at":"2021-02-09T00:00:00.000Z","note":"\u003cp\u003e26, local liom, friend, rust (lang) stan, bi \u003c/p\u003e\u003cp\u003eicon by \u003cspan class=\"h-card\"\u003e\u003ca href=\"https://furaffinity.net/user/lalupine\" target=\"blank\" rel=\"noopener noreferrer\" class=\"u-url mention\"\u003e@\u003cspan\u003elalupine@furaffinity.net\u003c/span\u003e\u003c/a\u003e\u003c/span\u003e\u003cbr /\u003eheader by \u003cspan class=\"h-card\"\u003e\u003ca href=\"https://furaffinity.net/user/tronixx\" target=\"blank\" rel=\"noopener noreferrer\" class=\"u-url mention\"\u003e@\u003cspan\u003etronixx@furaffinity.net\u003c/span\u003e\u003c/a\u003e\u003c/span\u003e\u003c/p\u003e\u003cp\u003eTestimonials:\u003c/p\u003e\u003cp\u003eStand: LIONS\u003cbr /\u003eStand User: AODE\u003cbr /\u003e- Keris (not on here)\u003c/p\u003e","url":"https://masto.asonix.dog/@asonix","avatar":"https://masto.asonix.dog/system/accounts/avatars/000/000/001/original/00852df0e6fee7e0.png","avatar_static":"https://masto.asonix.dog/system/accounts/avatars/000/000/001/original/00852df0e6fee7e0.png","header":"https://masto.asonix.dog/system/accounts/headers/000/000/001/original/8122ce3e5a745385.png","header_static":"https://masto.asonix.dog/system/accounts/headers/000/000/001/original/8122ce3e5a745385.png","followers_count":237,"following_count":474,"statuses_count":8798,"last_status_at":"2022-11-08","noindex":true,"emojis":[{"shortcode":"antiverified","url":"https://masto.asonix.dog/system/custom_emojis/images/000/030/053/original/bb0bc2e395b9a127.png","static_url":"https://masto.asonix.dog/system/custom_emojis/images/000/030/053/static/bb0bc2e395b9a127.png","visible_in_picker":true}],"fields":[{"name":"pronouns","value":"he/they","verified_at":null},{"name":"software","value":"bad","verified_at":null},{"name":"gitea","value":"\u003ca href=\"https://git.asonix.dog\" target=\"_blank\" rel=\"nofollow noopener noreferrer me\"\u003e\u003cspan class=\"invisible\"\u003ehttps://\u003c/span\u003e\u003cspan class=\"\"\u003egit.asonix.dog\u003c/span\u003e\u003cspan class=\"invisible\"\u003e\u003c/span\u003e\u003c/a\u003e","verified_at":null},{"name":"join my","value":"relay","verified_at":null}]},"rules":[]}"#;
    const HYNET_INSTANCE: &str = r#"{"approval_required":false,"avatar_upload_limit":2000000,"background_image":"https://soc.hyena.network/images/city.jpg","background_upload_limit":4000000,"banner_upload_limit":4000000,"description":"Akkoma: The cooler fediverse server","description_limit":5000,"email":"me@hyena.network","languages":["en"],"max_toot_chars":"5000","pleroma":{"metadata":{"account_activation_required":true,"features":["pleroma_api","mastodon_api","mastodon_api_streaming","polls","v2_suggestions","pleroma_explicit_addressing","shareable_emoji_packs","multifetch","pleroma:api/v1/notifications:include_types_filter","chat","shout","relay","safe_dm_mentions","pleroma_emoji_reactions","pleroma_chat_messages","exposable_reactions","profile_directory","custom_emoji_reactions"],"federation":{"enabled":true,"exclusions":false,"mrf_hashtag":{"federated_timeline_removal":[],"reject":[],"sensitive":["nsfw"]},"mrf_policies":["SimplePolicy","EnsureRePrepended","HashtagPolicy"],"mrf_simple":{"accept":[],"avatar_removal":[],"banner_removal":[],"federated_timeline_removal":["botsin.space"],"followers_only":[],"media_nsfw":["mstdn.jp","wxw.moe","knzk.me","vipgirlfriend.xxx","humblr.social","switter.at","kinkyelephant.com","sinblr.com","kinky.business","rubber.social"],"media_removal":[],"reject":["*.10minutepleroma.com","101010.pl","13bells.com","2.distsn.org","2hu.club","2ndamendment.social","434.earth","4chan.icu","4qq.org","7td.org","80percent.social","a.nti.social","aaathats3as.com","accela.online","amala.schwartzwelt.xyz","angrytoday.com","anime.website","antitwitter.moe","antivaxxer.icu","archivefedifor.fun","artalley.social","bae.st","bajax.us","baraag.net","bbs.kawa-kun.com","beefyboys.club","beefyboys.win","bikeshed.party","bitcoinhackers.org","bleepp.com","blovice.bahnhof.cz","brighteon.social","buildthatwallandmakeamericagreatagain.trumpislovetrumpis.life","bungle.online","cawfee.club","censorship.icu","chungus.cc","club.darknight-coffee.org","clubcyberia.co","cock.fish","cock.li","comfyboy.club","contrapointsfan.club","coon.town","counter.social","cum.salon","d-fens.systems","definitely-not-archivefedifor.fun","degenerates.fail","desuposter.club","detroitriotcity.com","developer.gab.com","dogwhipping.day","eientei.org","enigmatic.observer","eveningzoo.club","exited.eu","federation.krowverse.services","fedi.cc","fedi.krowverse.services","fedi.pawlicker.com","fedi.vern.cc","freak.university","freeatlantis.com","freecumextremist.com","freesoftwareextremist.com","freespeech.firedragonstudios.com","freespeech.host","freespeechextremist.com","freevoice.space","freezepeach.xyz","froth.zone","fuckgov.org","gab.ai","gab.polaris-1.work","gab.protohype.net","gabfed.com","gameliberty.club","gearlandia.haus","gitmo.life","glindr.org","glittersluts.xyz","glowers.club","godspeed.moe","gorf.pub","goyim.app","gs.kawa-kun.com","hagra.net","hallsofamenti.io","hayu.sh","hentai.baby","honkwerx.tech","hunk.city","husk.site","iddqd.social","ika.moe","isexychat.space","jaeger.website","justicewarrior.social","kag.social","katiehopkinspolitical.icu","kiwifarms.cc","kiwifarms.is","kiwifarms.net","kohrville.net","koyu.space","kys.moe","lain.com","lain.sh","leafposter.club","lets.saynoto.lgbt","liberdon.com","libertarianism.club","ligma.pro","lolis.world","masochi.st","masthead.social","mastodon.digitalsuccess.dev","mastodon.fidonet.io","mastodon.grin.hu","mastodon.ml","midnightride.rs","milker.cafe","mobile.tmediatech.io","moon.holiday","mstdn.foxfam.club","mstdn.io","mstdn.starnix.network","mulmeyun.church","nazi.social","neckbeard.xyz","neenster.org","neko.ci","netzsphaere.xyz","newjack.city","nicecrew.digital","nnia.space","noagendasocial.com","norrebro.space","oursocialism.today","ovo.sc","pawoo.net","paypig.org","pedo.school","phreedom.tk","pieville.net","pkteerium.xyz","pl.murky.club","pl.spiderden.net","pl.tkammer.de","pl.zombiecats.run","pleroma.nobodyhasthe.biz","pleroma.runfox.tk","pleroma.site","plr.inferencium.net","pmth.us","poa.st","pod.vladtepesblog.com","political.icu","pooper.social","posting.lolicon.rocks","preteengirls.biz","prout.social","qoto.org","rage.lol","rakket.app","raplst.town","rdrama.cc","ryona.agency","s.sneak.berlin","seal.cafe","sealion.club","search.fedi.app","sementerrori.st","shitposter.club","shortstackran.ch","silkhe.art","sleepy.cafe","soc.mahodou.moe","soc.redeyes.site","social.076.ne.jp","social.anoxinon.de","social.chadland.net","social.freetalklive.com","social.getgle.org","social.handholding.io","social.headsca.la","social.imirhil.fr","social.lovingexpressions.net","social.manalejandro.com","social.midwaytrades.com","social.pseudo-whiskey.bar","social.targaryen.house","social.teci.world","societal.co","society.oftrolls.com","socks.pinnoto.org","socnet.supes.com","solagg.com","spinster.xyz","springbo.cc","stereophonic.space","sunshinegardens.org","theautisticinvestors.quest","thechad.zone","theduran.icu","theosis.church","toot.love","toots.alirezahayati.com","traboone.com","truthsocial.co.in","truthsocial.com","tuusin.misono-ya.info","tweety.icu","unbound.social","unsafe.space","varishangout.net","video.nobodyhasthe.biz","voicenews.icu","voluntaryism.club","waifu.social","weeaboo.space","whinge.town","wolfgirl.bar","workers.dev","wurm.host","xiii.ch","xn--p1abe3d.xn--80asehdb","yggdrasil.social","youjo.love"],"reject_deletes":[],"report_removal":[]},"mrf_simple_info":{"federated_timeline_removal":{"botsin.space":{"reason":"A lot of bot content"}},"media_nsfw":{"humblr.social":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"kinky.business":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"kinkyelephant.com":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"knzk.me":{"reason":"Unmarked nsfw media"},"mstdn.jp":{"reason":"Not sure about the media policy"},"rubber.social":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"sinblr.com":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"switter.at":{"reason":"NSFW Instance, safe to assume most content is NSFW"},"vipgirlfriend.xxx":{"reason":"Unmarked nsfw media"},"wxw.moe":{"reason":"Unmarked nsfw media"}}},"quarantined_instances":[],"quarantined_instances_info":{"quarantined_instances":{}}},"fields_limits":{"max_fields":10,"max_remote_fields":20,"name_length":512,"value_length":2048},"post_formats":["text/plain","text/html","text/markdown","text/bbcode","text/x.misskeymarkdown"],"privileged_staff":false},"stats":{"mau":1},"vapid_public_key":"BMg4q-rT3rkMzc29F7OS5uM6t-Rx4HncMIB1NXrKwNlVRfX-W1kwgOuq5pDy-WhWmOZudaegftjBTCX3-pzdDFc"},"poll_limits":{"max_expiration":31536000,"max_option_chars":200,"max_options":20,"min_expiration":0},"registrations":"FALSE","shout_limit":5000,"stats":{"domain_count":1035,"status_count":7,"user_count":1},"thumbnail":"https://soc.hyena.network/instance/thumbnail.jpeg","title":"HyNET Social","upload_limit":16000000,"uri":"https://soc.hyena.network","urls":{"streaming_api":"wss://soc.hyena.network"},"version":"2.7.2 (compatible; Akkoma 3.0.0)"}"#;
    const MISSKEY_BARE_INSTANCE: &str = r#"{"maintainerName":null,"maintainerEmail":null,"version":"12.119.2","name":null,"uri":"https://msky-lab-01.arewesecureyet.org","description":null,"langs":[],"tosUrl":null,"repositoryUrl":"https://github.com/misskey-dev/misskey","feedbackUrl":"https://github.com/misskey-dev/misskey/issues/new","disableRegistration":false,"disableLocalTimeline":false,"disableGlobalTimeline":false,"driveCapacityPerLocalUserMb":1024,"driveCapacityPerRemoteUserMb":32,"emailRequiredForSignup":false,"enableHcaptcha":false,"hcaptchaSiteKey":null,"enableRecaptcha":false,"recaptchaSiteKey":null,"swPublickey":null,"themeColor":null,"mascotImageUrl":"/assets/ai.png","bannerUrl":null,"errorImageUrl":"https://xn--931a.moe/aiart/yubitun.png","iconUrl":null,"backgroundImageUrl":null,"logoImageUrl":null,"maxNoteTextLength":3000,"emojis":[],"defaultLightTheme":null,"defaultDarkTheme":null,"ads":[],"enableEmail":false,"enableTwitterIntegration":false,"enableGithubIntegration":false,"enableDiscordIntegration":false,"enableServiceWorker":false,"translatorAvailable":false,"pinnedPages":["/featured","/channels","/explore","/pages","/about-misskey"],"pinnedClipId":null,"cacheRemoteFiles":true,"requireSetup":false,"proxyAccountName":null,"features":{"registration":true,"localTimeLine":true,"globalTimeLine":true,"emailRequiredForSignup":false,"elasticsearch":false,"hcaptcha":false,"recaptcha":false,"objectStorage":false,"twitter":false,"github":false,"discord":false,"serviceWorker":false,"miauth":true}}"#;
    const MISSKEY_STELLA_INSTANCE: &str = r###"{"maintainerName":"Caipira","maintainerEmail":"caipira@sagestn.one","version":"12.120.0-alpha.8+cs","name":"Stella","uri":"https://stella.place","description":"<center>무수히 많은 별 중 하나인 인스턴스입니다.</center>\n<center>SINCE 2022. 5. 8. ~</center>\n<center>|</center>\n<b><center>Enable</center></b>\n<small>Castella = Misskey fork (.....)</small>","langs":[],"tosUrl":"https://docs.stella.place/tos","repositoryUrl":"https://github.com/misskey-dev/misskey","feedbackUrl":"https://github.com/misskey-dev/misskey/issues/new","disableRegistration":false,"disableLocalTimeline":false,"disableGlobalTimeline":false,"driveCapacityPerLocalUserMb":3072,"driveCapacityPerRemoteUserMb":32,"emailRequiredForSignup":true,"enableHcaptcha":true,"hcaptchaSiteKey":"94d629f6-a38e-4f24-83dd-63326c7e3bbf","enableRecaptcha":false,"recaptchaSiteKey":"6Lf-9dIfAAAAAF0Jp_QSsIlltyi371ZSU48Csisy","enableTurnstile":false,"turnstileSiteKey":"0x4AAAAAAAArrDq-OcfsyU-R","swPublickey":"BNTI1ms29LPGpdF8spPKa5khs6B2UYnVWa3KcO6e6JJoVXzbCBjdUdpkZHo-MK_AZfJxbTE8Z8C7g5kQChEkfp8","themeColor":"#df99f7","mascotImageUrl":"/assets/ai.png","bannerUrl":"https://cdn.stella.place/assets/bg.jpg","errorImageUrl":"https://xn--931a.moe/aiart/yubitun.png","iconUrl":"https://cdn.stella.place/assets/Stella.png","backgroundImageUrl":"https://cdn.stella.place/assets/bg.jpg","logoImageUrl":null,"maxNoteTextLength":3000,"emojis":[{"id":"97f9mubsmt","aliases":[""],"name":"big_blobhaj_hug","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/15d27c96-093a-4dc0-ae57-b848180b073b.png"},{"id":"97f9rzt5ok","aliases":[""],"name":"blobhaj","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/fb06df30-00d8-4b3b-ac8c-ee779ff06599.png"},{"id":"97f9rzjso5","aliases":[""],"name":"blobhaj_asparagus","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/35ed1236-8ebc-4c91-a793-0cfd04532148.png"},{"id":"97f9p8gtn3","aliases":[""],"name":"blobhaj_blanket_blue","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/d5876a69-1dd3-4e6a-b3cd-d497c06064fa.png"},{"id":"97f9p8gonf","aliases":[""],"name":"blobhaj_blanket_slate","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/43c461e7-f8af-4636-b72a-6d0ef40c2655.png"},{"id":"97f9rzl7nt","aliases":[""],"name":"blobhaj_blobby_hug","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/8a272ac5-b9d7-4554-ab18-7088f6132bc3.png"},{"id":"97f9rzmjo7","aliases":[""],"name":"blobhaj_full_body_hug","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/9f8251d0-c816-441e-9eef-2687502023fc.png"},{"id":"97f9rzl0ns","aliases":[""],"name":"blobhaj_heart","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/48ec936e-ae27-4b7c-94db-b673b628e231.png"},{"id":"97f9gwvvl8","aliases":[""],"name":"blobhaj_mdbook","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/56fdbaa3-06e6-4f5d-81d6-3e8e54ae059c.png"},{"id":"97f9rzk7nq","aliases":[""],"name":"blobhaj_mlem","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/03ea3ae0-2d40-4faf-8d3c-10f788a3de1d.png"},{"id":"97f9fpovkq","aliases":[""],"name":"blobhaj_octobook","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/6e992574-d8ca-4592-80a0-4f7bdff03cf8.png"},{"id":"97f9rzpjoc","aliases":[""],"name":"blobhaj_pride_heart","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/c397da98-dd44-484e-8de4-7db11a677e42.png"},{"id":"97f9rzjko4","aliases":[""],"name":"blobhaj_reach","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/e2c00652-a2f6-4ccf-bf0e-796f85b7854d.png"},{"id":"97f9rzjxo6","aliases":[""],"name":"blobhaj_sad_reach","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/bf36e678-20fd-49cc-8485-3ae5b8db033e.png"},{"id":"97f9rzqxod","aliases":[""],"name":"blobhaj_shock","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/ab9a3062-3bda-48c3-a91a-95beb86d56cc.png"},{"id":"97f9rzo0oa","aliases":[""],"name":"blobhaj_tiny_heart","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/aec32c7c-2b53-4251-b3ba-862587fa6ff6.png"},{"id":"97f9rznto8","aliases":[""],"name":"blobhaj_trans_pride_heart","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/e0dad2d0-9136-4310-8841-7bf3819ba152.png"},{"id":"97f9k7iqm2","aliases":[""],"name":"sir_blobhaj","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/70546774-e704-4a93-ab28-059ef3c54b65.png"},{"id":"97f9ikojll","aliases":[""],"name":"sir_blobhaj_stern","category":"Blobhaj Hub","host":null,"url":"https://cdn.stella.place/D1/2d69a377-a562-4769-8f60-b3ab8794b2a2.png"},{"id":"953ytptaah","aliases":[""],"name":"ysoyaboom","category":"Blue Archive","host":null,"url":"https://cdn.stella.place/D1/51240ea1-345e-4d43-a293-300dc968197a.gif"},{"id":"953ytpt8ag","aliases":[""],"name":"ysoyasnap","category":"Blue Archive","host":null,"url":"https://cdn.stella.place/D1/e95337b8-499b-47b2-811e-e1f4398efc69.gif"},{"id":"97q78i6ss2","aliases":[""],"name":"terminal_blinker","category":"Dev","host":null,"url":"https://cdn.stella.place/D1/a04022ea-d8dc-4861-a7fd-360d178bd01f.png"},{"id":"97v0x375vq","aliases":[],"name":"ff14_msq","category":"FF14","host":null,"url":"https://cdn.stella.place/D1/db14bccd-8f89-428b-b1b9-8da7e821656b.png"},{"id":"97v0x375jq","aliases":[],"name":"ff14_msq_done","category":"FF14","host":null,"url":"https://cdn.stella.place/D1/7be4bb6d-9271-4e40-abaf-24fe97ec9a76.png"},{"id":"97v0x37yjs","aliases":[],"name":"ff14_msq_locked","category":"FF14","host":null,"url":"https://cdn.stella.place/D1/e2c85711-032b-4a83-b933-24e8a96ece82.png"},{"id":"9542b9c4io","aliases":["사이노"],"name":"cyno_oops","category":"Genshin Impact","host":null,"url":"https://cdn.stella.place/D1/2507ad57-b042-407d-a84e-e676ea8710c5.gif"},{"id":"97lo6lbcin","aliases":["합니다"],"name":"kr_doing","category":"KR","host":null,"url":"https://cdn.stella.place/D1/eeee5f16-ac99-42e7-8c4c-c773150d4bd5.png"},{"id":"97lo6u3zqa","aliases":["정상"],"name":"kr_normal","category":"KR","host":null,"url":"https://cdn.stella.place/D1/29e82750-da88-4baf-a8ec-64b4bcafdf28.png"},{"id":"97lo7523j2","aliases":["우리"],"name":"kr_our","category":"KR","host":null,"url":"https://cdn.stella.place/D1/458304e1-72e3-4fd2-87d4-c05a6a152083.png"},{"id":"97lo6zujj0","aliases":["식당"],"name":"kr_restaurant","category":"KR","host":null,"url":"https://cdn.stella.place/D1/e257d140-e8fe-477a-b5be-4e7e29239544.png"},{"id":"97lo6q34ir","aliases":["영업"],"name":"kr_sales","category":"KR","host":null,"url":"https://cdn.stella.place/D1/2331cb3a-5ca5-42ae-b677-691f8fe3b4e4.png"},{"id":"96kc58706a","aliases":[""],"name":"Kirkland_Signature","category":"Logo","host":null,"url":"https://cdn.stella.place/D1/b24c722a-0cf3-485c-8bb6-e8385e2dcd2b.png"},{"id":"95dwescv8x","aliases":[""],"name":"ms_angry","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/08407ba0-eb70-4385-ad7e-7987a1b178b8.png"},{"id":"95dwdph489","aliases":[""],"name":"ms_balloon","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/d3e49618-d9fb-4142-b369-b8b0a19f6875.png"},{"id":"95dwcb0w7c","aliases":[""],"name":"ms_contempt","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/b89d5827-d4ca-40d8-b250-02de2ab44ed9.png"},{"id":"95dwe68v8j","aliases":[""],"name":"ms_lie","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/889a793d-07a7-4f1f-a6a8-a151ed4ce1e3.png"},{"id":"95dwa1yc5x","aliases":[""],"name":"ms_mastodon","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/75fce83f-50a4-4ae9-86f3-52882279ccfd.png"},{"id":"95dwgcp0a8","aliases":[""],"name":"ms_messgaki","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/a432b00e-0ba0-4e13-aa25-1b6b61469807.png"},{"id":"95dwfs4h9q","aliases":[""],"name":"ms_messgaki_normal","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/5b9e1a10-f4af-4d35-af4f-a1cca7df5454.png"},{"id":"95dwb9i26m","aliases":[""],"name":"ms_messtodon","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/a2c7ff09-3090-445d-be8b-68144ac59e5a.png"},{"id":"95dwdbym81","aliases":[""],"name":"ms_sleepy","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/bbf5648e-6deb-4f3d-9fc0-6f9d1bbccd40.png"},{"id":"95dwf76i98","aliases":[""],"name":"ms_small_messgaki","category":"Messtodon","host":null,"url":"https://cdn.stella.place/D1/430b9d2a-591e-4beb-a037-fea66c777319.png"},{"id":"97pjdnpp76","aliases":["42"],"name":"text_42","category":"TEXT","host":null,"url":"https://cdn.stella.place/D1/e253653a-94b0-4cad-9277-f9c8e1188c5f.png"},{"id":"96njxnbkgt","aliases":[""],"name":"yt_social_circle_dark","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-0e0615fe-c430-43c2-910b-c9eea6ebf5ab.png"},{"id":"96njxneyh4","aliases":[""],"name":"yt_social_circle_red","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-7780f3d7-c0ed-46c8-93f2-afa01c39c65e.png"},{"id":"96njxnazgp","aliases":[""],"name":"yt_social_circle_white","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-ce023618-077a-4e80-8c37-827bd1490803.png"},{"id":"96njxnb8gr","aliases":[""],"name":"yt_social_square_dark","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-c48f67f6-6b46-4101-a8ba-123f5fa2508d.png"},{"id":"96njxnc7gv","aliases":[""],"name":"yt_social_square_red","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-51e24c3e-d8a7-4f52-b342-718823fc9631.png"},{"id":"96njxngrh7","aliases":[""],"name":"yt_social_square_white","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-de69c7e1-7044-4eab-b53f-7629342bcb11.png"},{"id":"96njxnbags","aliases":[""],"name":"yt_social_squircle_dark","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-de763d0f-3659-463e-b9b4-f6975a007b37.png"},{"id":"96njxnh5ha","aliases":[""],"name":"yt_social_squircle_red","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-a7e847b3-23ad-40ad-a06e-f6eafe23e001.png"},{"id":"96njxnb4gq","aliases":[""],"name":"yt_social_squircle_white","category":"YouTube Brand resources","host":null,"url":"https://cdn.stella.place/D1/webpublic-7d1878a0-e2a8-4336-a6f8-094a4b44a759.png"},{"id":"937z342qj0","aliases":[""],"name":"blackverified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/496d2618-4a32-41a8-a1ba-df798a16f209.gif"},{"id":"937zd787m6","aliases":[""],"name":"blueviolet_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/357c3a1e-88d0-47f3-9812-5f1f15dec382.gif"},{"id":"937z127ui9","aliases":[""],"name":"freshair_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/7d10a1b1-8128-4400-af63-f28d4c3c9888.gif"},{"id":"937zkingmk","aliases":[""],"name":"fuchsia_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/5cf05079-8216-40ca-875d-8cacb546e1f9.gif"},{"id":"937zhe1mmg","aliases":[""],"name":"lightred_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/abd3c0f9-d046-4549-9d1e-f018d92982bf.gif"},{"id":"937zfpirmc","aliases":[""],"name":"mangotango_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/9c4de8ba-2c63-41bb-b581-e3e52e2017be.gif"},{"id":"937ziz3lmi","aliases":[""],"name":"middleyellow_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/86a74a5f-51e9-40f8-b2fd-de56927b72b0.gif"},{"id":"937zlhbumm","aliases":[""],"name":"palelavender_verifiedX","category":"verified","host":null,"url":"https://cdn.stella.place/D1/6364fb55-8c48-49b9-9cb9-2a06f50a920a.gif"},{"id":"937xqp173v","aliases":[""],"name":"pink_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/41f5eb90-86f5-4955-9de5-5439293b2a90.gif"},{"id":"937z46vbje","aliases":[""],"name":"red_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/bac2a25a-b79b-42db-bc75-f398dc18b2d1.gif"},{"id":"937yrlcvfv","aliases":[""],"name":"turquoise_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/2ff8d332-5563-4951-a2dc-c627fc5c1ff1.gif"},{"id":"937atf86py","aliases":[""],"name":"verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/654525f0-a8f3-415f-9051-c79739f72c31.gif"},{"id":"937yzdnihz","aliases":[""],"name":"vividred_verified","category":"verified","host":null,"url":"https://cdn.stella.place/D1/3d06e1ef-372b-4d22-9d80-02e836b911b1.gif"},{"id":"98px6gj0a9","aliases":["엘렐레"],"name":"gardeneel_ellelle","category":null,"host":null,"url":"https://cdn.stella.place/D1/c5810e07-57ad-4fd8-ae86-753e2dd799d1.png"},{"id":"98pw3pndpo","aliases":["메롱","엘렐레"],"name":"gardeneel_tongue","category":null,"host":null,"url":"https://cdn.stella.place/D1/877b2ca3-6540-4d77-a061-f1a5e047e147.png"},{"id":"97rs46kkuz","aliases":["자빠"],"name":"yum_1","category":null,"host":null,"url":"https://cdn.stella.place/D1/74d00105-84fc-4050-8bd0-28d16abba3dd.png"},{"id":"97rs4w2ubs","aliases":["지게"],"name":"yum_2","category":null,"host":null,"url":"https://cdn.stella.place/D1/8cd4b526-d42f-42af-b3a5-6f6502798866.png"},{"id":"97rs593z6t","aliases":["맛있ㅇ"],"name":"yum_3","category":null,"host":null,"url":"https://cdn.stella.place/D1/095eba74-7862-4719-aa09-5ad28aa7d29a.png"},{"id":"97rs5lph70","aliases":["ㅓ요"],"name":"yum_4","category":null,"host":null,"url":"https://cdn.stella.place/D1/1d28ee5a-dd45-4fb0-bb65-a96ee20ed971.png"}],"defaultLightTheme":null,"defaultDarkTheme":null,"ads":[],"enableEmail":true,"enableTwitterIntegration":false,"enableGithubIntegration":false,"enableDiscordIntegration":true,"enableServiceWorker":true,"translatorAvailable":false,"pinnedPages":["/featured","/channels","/explore","/pages","/about-misskey"],"pinnedClipId":null,"cacheRemoteFiles":true,"requireSetup":false,"proxyAccountName":"proxy","features":{"registration":true,"localTimeLine":true,"globalTimeLine":true,"emailRequiredForSignup":true,"elasticsearch":false,"hcaptcha":true,"recaptcha":false,"turnstile":false,"objectStorage":true,"twitter":false,"github":false,"discord":true,"serviceWorker":true,"miauth":true}}"###;

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

    #[test]
    fn deser_misskey_instance_without_contact() {
        let meta: MisskeyMeta = serde_json::from_str(MISSKEY_BARE_INSTANCE).unwrap();
        assert!(meta.icon_url.is_none());
    }

    #[test]
    fn deser_misskey_instance_with_contact() {
        let meta: MisskeyMeta = serde_json::from_str(MISSKEY_STELLA_INSTANCE).unwrap();
        assert_eq!(
            meta.icon_url.unwrap(),
            "https://cdn.stella.place/assets/Stella.png"
        );
    }

    #[test]
    fn deser_misskey_instance_into() {
        let meta: MisskeyMeta = serde_json::from_str(MISSKEY_STELLA_INSTANCE).unwrap();
        let inst: Instance = meta.into();

        assert_eq!(
            inst.contact.unwrap().avatar,
            "https://cdn.stella.place/assets/Stella.png"
        );
    }
}
