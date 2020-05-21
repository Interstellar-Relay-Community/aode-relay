use crate::{
    config::{Config, UrlKind},
    data::{Actor, State},
    error::MyError,
};
use activitystreams_new::{
    activity::{Follow as AsFollow, Undo as AsUndo},
    context,
    prelude::*,
    primitives::XsdAnyUri,
    security,
};
use std::convert::TryInto;

mod announce;
mod follow;
mod forward;
mod reject;
mod undo;

pub use self::{announce::Announce, follow::Follow, forward::Forward, reject::Reject, undo::Undo};

async fn get_inboxes(
    state: &State,
    actor: &Actor,
    object_id: &XsdAnyUri,
) -> Result<Vec<XsdAnyUri>, MyError> {
    let domain = object_id
        .as_url()
        .host()
        .ok_or(MyError::Domain)?
        .to_string();

    Ok(state.listeners_without(&actor.inbox, &domain).await)
}

fn prepare_activity<T, U, V, Kind>(
    mut t: T,
    id: impl TryInto<XsdAnyUri, Error = U>,
    to: impl TryInto<XsdAnyUri, Error = V>,
) -> Result<T, MyError>
where
    T: ObjectExt<Kind> + BaseExt<Kind>,
    MyError: From<U> + From<V>,
{
    t.set_id(id.try_into()?)
        .set_many_tos(vec![to.try_into()?])
        .set_many_contexts(vec![context(), security()]);
    Ok(t)
}

// Generate a type that says "I want to stop following you"
fn generate_undo_follow(
    config: &Config,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<AsUndo, MyError> {
    let mut follow = AsFollow::new(my_id.clone(), actor_id.clone());

    follow.set_id(config.generate_url(UrlKind::Activity).parse()?);

    let undo = AsUndo::new(my_id.clone(), follow.into_any_base()?);

    prepare_activity(undo, config.generate_url(UrlKind::Actor), actor_id.clone())
}
