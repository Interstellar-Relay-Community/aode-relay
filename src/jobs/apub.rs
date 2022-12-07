use crate::{
    config::{Config, UrlKind},
    data::State,
    db::Actor,
    error::{Error, ErrorKind},
};
use activitystreams::{
    activity::{Follow as AsFollow, Undo as AsUndo},
    context,
    iri_string::types::IriString,
    prelude::*,
    security,
};
use std::convert::TryInto;

mod announce;
mod follow;
mod forward;
mod reject;
mod undo;

pub(crate) use self::{
    announce::Announce, follow::Follow, forward::Forward, reject::Reject, undo::Undo,
};

async fn get_inboxes(
    state: &State,
    actor: &Actor,
    object_id: &IriString,
) -> Result<Vec<IriString>, Error> {
    let authority = object_id
        .authority_str()
        .ok_or(ErrorKind::Domain)?
        .to_string();

    state.inboxes_without(&actor.inbox, &authority).await
}

fn prepare_activity<T, U, V>(
    mut t: T,
    id: impl TryInto<IriString, Error = U>,
    to: impl TryInto<IriString, Error = V>,
) -> Result<T, Error>
where
    T: ObjectExt + BaseExt,
    Error: From<U> + From<V>,
{
    t.set_id(id.try_into()?)
        .set_many_tos(vec![to.try_into()?])
        .set_many_contexts(vec![context(), security()]);
    Ok(t)
}

// Generate a type that says "I want to stop following you"
fn generate_undo_follow(
    config: &Config,
    actor_id: &IriString,
    my_id: &IriString,
) -> Result<AsUndo, Error> {
    let mut follow = AsFollow::new(my_id.clone(), actor_id.clone());

    follow.set_id(config.generate_url(UrlKind::Activity));

    let undo = AsUndo::new(my_id.clone(), follow.into_any_base()?);

    prepare_activity(undo, config.generate_url(UrlKind::Actor), actor_id.clone())
}
