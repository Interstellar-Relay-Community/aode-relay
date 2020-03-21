use activitystreams::primitives::XsdAnyUriError;
use actix_web::{
    error::{BlockingError, ResponseError},
    http::StatusCode,
    HttpResponse,
};
use log::error;
use rsa_pem::KeyError;
use std::{convert::Infallible, fmt::Debug, io::Error};

#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("Error queueing job, {0}")]
    Queue(anyhow::Error),

    #[error("Error in configuration, {0}")]
    Config(#[from] config::ConfigError),

    #[error("Error in db, {0}")]
    DbError(#[from] bb8_postgres::tokio_postgres::error::Error),

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

    #[error("Signature verification is required, but no signature was given")]
    NoSignature(String),

    #[error("Wrong ActivityPub kind, {0}")]
    Kind(String),

    #[error("Too many CPUs, {0}")]
    CpuCount(#[from] std::num::TryFromIntError),

    #[error("Couldn't flush buffer")]
    FlushBuffer,

    #[error("Timed out while waiting on db pool")]
    DbTimeout,

    #[error("Invalid algorithm provided to verifier")]
    Algorithm,

    #[error("Object has already been relayed")]
    Duplicate,

    #[error("Couldn't send request")]
    SendRequest,

    #[error("Couldn't receive request response")]
    ReceiveResponse,

    #[error("Response has invalid status code, {0}")]
    Status(StatusCode),

    #[error("URI is missing domain field")]
    Domain,

    #[error("Blocking operation was canceled")]
    Canceled,
}

impl ResponseError for MyError {
    fn status_code(&self) -> StatusCode {
        match self {
            MyError::Blocked(_)
            | MyError::Whitelist(_)
            | MyError::WrongActor(_)
            | MyError::BadActor(_, _) => StatusCode::FORBIDDEN,
            MyError::NotSubscribed(_) => StatusCode::UNAUTHORIZED,
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

impl<T> From<BlockingError<T>> for MyError
where
    T: Into<MyError> + Debug,
{
    fn from(e: BlockingError<T>) -> Self {
        match e {
            BlockingError::Error(e) => e.into(),
            BlockingError::Canceled => MyError::Canceled,
        }
    }
}

impl<T> From<bb8_postgres::bb8::RunError<T>> for MyError
where
    T: Into<MyError>,
{
    fn from(e: bb8_postgres::bb8::RunError<T>) -> Self {
        match e {
            bb8_postgres::bb8::RunError::User(e) => e.into(),
            bb8_postgres::bb8::RunError::TimedOut => MyError::DbTimeout,
        }
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
