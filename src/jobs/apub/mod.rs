use crate::{
    config::{Config, UrlKind},
    data::{Actor, State},
    error::MyError,
};
use activitystreams::{
    context, object::properties::ObjectProperties, primitives::XsdAnyUri, security,
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

fn prepare_activity<T, U, V>(
    mut t: T,
    id: impl TryInto<XsdAnyUri, Error = U>,
    to: impl TryInto<XsdAnyUri, Error = V>,
) -> Result<T, MyError>
where
    T: AsMut<ObjectProperties>,
    MyError: From<U> + From<V>,
{
    t.as_mut()
        .set_id(id.try_into()?)?
        .set_many_to_xsd_any_uris(vec![to.try_into()?])?
        .set_many_context_xsd_any_uris(vec![context(), security()])?;
    Ok(t)
}

// Generate a type that says "I want to stop following you"
fn generate_undo_follow(
    config: &Config,
    actor_id: &XsdAnyUri,
    my_id: &XsdAnyUri,
) -> Result<activitystreams::activity::Undo, MyError> {
    let mut undo = activitystreams::activity::Undo::default();

    undo.undo_props
        .set_actor_xsd_any_uri(my_id.clone())?
        .set_object_base_box({
            let mut follow = activitystreams::activity::Follow::default();

            follow
                .object_props
                .set_id(config.generate_url(UrlKind::Activity))?;
            follow
                .follow_props
                .set_actor_xsd_any_uri(actor_id.clone())?
                .set_object_xsd_any_uri(actor_id.clone())?;

            follow
        })?;

    prepare_activity(undo, config.generate_url(UrlKind::Actor), actor_id.clone())
}
