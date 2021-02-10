use activitystreams::{error::DomainError, url::ParseError};
use actix_web::{
    error::{BlockingError, ResponseError},
    http::StatusCode,
    HttpResponse,
};
use http_signature_normalization_actix::PrepareSignError;
use log::error;
use rsa_pem::KeyError;
use std::{convert::Infallible, fmt::Debug, io::Error};

#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("Error queueing job, {0}")]
    Queue(anyhow::Error),

    #[error("Error in configuration, {0}")]
    Config(#[from] config::ConfigError),

    #[error("Couldn't parse key, {0}")]
    Key(#[from] KeyError),

    #[error("Couldn't parse URI, {0}")]
    Uri(#[from] ParseError),

    #[error("Couldn't perform IO, {0}")]
    Io(#[from] Error),

    #[error("Couldn't sign string, {0}")]
    Rsa(rsa::errors::Error),

    #[error("Couldn't use db, {0}")]
    Sled(#[from] sled::Error),

    #[error("Couldn't do the json thing, {0}")]
    Json(#[from] serde_json::Error),

    #[error("Couldn't build signing string, {0}")]
    PrepareSign(#[from] PrepareSignError),

    #[error("Couldn't parse the signature header")]
    HeaderValidation(#[from] actix_web::http::header::InvalidHeaderValue),

    #[error("Couldn't decode base64")]
    Base64(#[from] base64::DecodeError),

    #[error("Actor ({0}), or Actor's server, is not subscribed")]
    NotSubscribed(String),

    #[error("Actor is not allowed, {0}")]
    NotAllowed(String),

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

    #[error("{0}")]
    HostMismatch(#[from] DomainError),

    #[error("Invalid or missing content type")]
    ContentType,

    #[error("Couldn't flush buffer")]
    FlushBuffer,

    #[error("Invalid algorithm provided to verifier, {0}")]
    Algorithm(String),

    #[error("Object has already been relayed")]
    Duplicate,

    #[error("Couldn't send request to {0}, {1}")]
    SendRequest(String, String),

    #[error("Couldn't receive request response from {0}, {1}")]
    ReceiveResponse(String, String),

    #[error("Response from {0} has invalid status code, {1}")]
    Status(String, StatusCode),

    #[error("Uri {0} is missing host")]
    Host(String),

    #[error("Expected an Object, found something else")]
    ObjectFormat,

    #[error("Expected a single object, found array")]
    ObjectCount,

    #[error("Input is missing a 'type' field")]
    MissingKind,

    #[error("Input is missing a 'id' field")]
    MissingId,

    #[error("Url is missing a domain")]
    MissingDomain,

    #[error("URI is missing domain field")]
    Domain,

    #[error("Blocking operation was canceled")]
    Canceled,

    #[error("Not trying request due to failed breaker")]
    Breaker,
}

impl ResponseError for MyError {
    fn status_code(&self) -> StatusCode {
        match self {
            MyError::NotAllowed(_) | MyError::WrongActor(_) | MyError::BadActor(_, _) => {
                StatusCode::FORBIDDEN
            }
            MyError::NotSubscribed(_) => StatusCode::UNAUTHORIZED,
            MyError::Duplicate => StatusCode::ACCEPTED,
            MyError::Kind(_) | MyError::MissingKind | MyError::MissingId | MyError::ObjectCount => {
                StatusCode::BAD_REQUEST
            }
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
