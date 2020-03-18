use activitystreams::primitives::XsdAnyUriError;
use actix::MailboxError;
use actix_web::{error::ResponseError, http::StatusCode, HttpResponse};
use log::error;
use rsa_pem::KeyError;
use std::{convert::Infallible, io::Error};
use tokio::sync::oneshot::error::RecvError;

#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("Couldn't parse key, {0}")]
    Key(#[from] KeyError),

    #[error("Couldn't parse URI, {0}")]
    Uri(#[from] XsdAnyUriError),

    #[error("Couldn't perform IO, {0}")]
    Io(#[from] Error),

    #[error("Couldn't sign string")]
    Rsa(rsa::errors::Error),

    #[error("Couldn't do the json thing")]
    Json(#[from] serde_json::Error),

    #[error("Couldn't serialzize the signature header")]
    HeaderSerialize(#[from] actix_web::http::header::ToStrError),

    #[error("Couldn't parse the signature header")]
    HeaderValidation(#[from] actix_web::http::header::InvalidHeaderValue),

    #[error("Failed to get output of db operation")]
    Oneshot(#[from] RecvError),

    #[error("Couldn't decode base64")]
    Base64(#[from] base64::DecodeError),

    #[error("Actor ({0}), or Actor's server, is not subscribed")]
    NotSubscribed(String),

    #[error("Actor is blocked, {0}")]
    Blocked(String),

    #[error("Actor is not whitelisted, {0}")]
    Whitelist(String),

    #[error("Cannot make decisions for foreign actor, {0}")]
    WrongActor(String),

    #[error("Actor ({0}) tried to submit another actor's ({1}) payload")]
    BadActor(String, String),

    #[error("Wrong ActivityPub kind, {0}")]
    Kind(String),

    #[error("The requested actor's mailbox is closed")]
    MailboxClosed,

    #[error("The requested actor's mailbox has timed out")]
    MailboxTimeout,

    #[error("Invalid algorithm provided to verifier")]
    Algorithm,

    #[error("Object has already been relayed")]
    Duplicate,

    #[error("Couldn't send request")]
    SendRequest,

    #[error("Couldn't receive request response")]
    ReceiveResponse,

    #[error("Response has invalid status code")]
    Status,

    #[error("URI is missing domain field")]
    Domain,
}

impl ResponseError for MyError {
    fn status_code(&self) -> StatusCode {
        match self {
            MyError::Blocked(_)
            | MyError::Whitelist(_)
            | MyError::WrongActor(_)
            | MyError::BadActor(_, _) => StatusCode::FORBIDDEN,
            MyError::Duplicate => StatusCode::ACCEPTED,
            MyError::Kind(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .header("Content-Type", "application/activity+json")
            .json(serde_json::json!({
                "error": self.to_string(),
            }))
    }
}

impl From<Infallible> for MyError {
    fn from(i: Infallible) -> Self {
        match i {}
    }
}

impl From<rsa::errors::Error> for MyError {
    fn from(e: rsa::errors::Error) -> Self {
        MyError::Rsa(e)
    }
}

impl From<MailboxError> for MyError {
    fn from(m: MailboxError) -> MyError {
        match m {
            MailboxError::Closed => MyError::MailboxClosed,
            MailboxError::Timeout => MyError::MailboxTimeout,
        }
    }
}
