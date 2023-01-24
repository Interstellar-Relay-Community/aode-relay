use activitystreams::checked::CheckError;
use actix_rt::task::JoinError;
use actix_web::{
    error::{BlockingError, ResponseError},
    http::StatusCode,
    HttpResponse,
};
use http_signature_normalization_actix::PrepareSignError;
use std::{convert::Infallible, fmt::Debug, io};
use tracing_error::SpanTrace;

pub(crate) struct Error {
    context: String,
    kind: ErrorKind,
}

impl Error {
    pub(crate) fn is_breaker(&self) -> bool {
        matches!(self.kind, ErrorKind::Breaker)
    }

    pub(crate) fn is_not_found(&self) -> bool {
        matches!(self.kind, ErrorKind::Status(_, StatusCode::NOT_FOUND))
    }

    pub(crate) fn is_bad_request(&self) -> bool {
        matches!(self.kind, ErrorKind::Status(_, StatusCode::BAD_REQUEST))
    }

    pub(crate) fn is_gone(&self) -> bool {
        matches!(self.kind, ErrorKind::Status(_, StatusCode::GONE))
    }

    pub(crate) fn is_malformed_json(&self) -> bool {
        matches!(self.kind, ErrorKind::Json(_))
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{:?}", self.kind)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.kind)?;
        std::fmt::Display::fmt(&self.context, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.kind.source()
    }
}

impl<T> From<T> for Error
where
    ErrorKind: From<T>,
{
    fn from(error: T) -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: error.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ErrorKind {
    #[error("Error queueing job, {0}")]
    Queue(anyhow::Error),

    #[error("Error in configuration, {0}")]
    Config(#[from] config::ConfigError),

    #[error("Couldn't parse key, {0}")]
    Pkcs8(#[from] rsa::pkcs8::Error),

    #[error("Couldn't encode public key, {0}")]
    Spki(#[from] rsa::pkcs8::spki::Error),

    #[error("Couldn't parse IRI, {0}")]
    ParseIri(#[from] activitystreams::iri_string::validate::Error),

    #[error("Couldn't normalize IRI, {0}")]
    NormalizeIri(#[from] std::collections::TryReserveError),

    #[error("Couldn't perform IO, {0}")]
    Io(#[from] io::Error),

    #[error("Couldn't sign string, {0}")]
    Rsa(rsa::errors::Error),

    #[error("Couldn't use db, {0}")]
    Sled(#[from] sled::Error),

    #[error("Couldn't do the json thing, {0}")]
    Json(#[from] serde_json::Error),

    #[error("Couldn't build signing string, {0}")]
    PrepareSign(#[from] PrepareSignError),

    #[error("Couldn't sign digest")]
    Signature(#[from] rsa::signature::Error),

    #[error("Couldn't read signature")]
    ReadSignature(rsa::signature::Error),

    #[error("Couldn't verify signature")]
    VerifySignature(rsa::signature::Error),

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
    NoSignature(Option<String>),

    #[error("Wrong ActivityPub kind, {0}")]
    Kind(String),

    #[error("Too many CPUs, {0}")]
    CpuCount(#[from] std::num::TryFromIntError),

    #[error("{0}")]
    HostMismatch(#[from] CheckError),

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

    #[error("Expected an Object, found something else")]
    ObjectFormat,

    #[error("Expected a single object, found array")]
    ObjectCount,

    #[error("Input is missing a 'type' field")]
    MissingKind,

    #[error("Input is missing a 'id' field")]
    MissingId,

    #[error("IriString is missing a domain")]
    MissingDomain,

    #[error("URI is missing domain field")]
    Domain,

    #[error("Blocking operation was canceled")]
    Canceled,

    #[error("Not trying request due to failed breaker")]
    Breaker,

    #[error("Failed to extract fields from {0}")]
    Extract(&'static str),

    #[error("No API Token supplied")]
    MissingApiToken,
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match self.kind {
            ErrorKind::NotAllowed(_) | ErrorKind::WrongActor(_) | ErrorKind::BadActor(_, _) => {
                StatusCode::FORBIDDEN
            }
            ErrorKind::NotSubscribed(_) => StatusCode::UNAUTHORIZED,
            ErrorKind::Duplicate => StatusCode::ACCEPTED,
            ErrorKind::Kind(_)
            | ErrorKind::MissingKind
            | ErrorKind::MissingId
            | ErrorKind::ObjectCount
            | ErrorKind::NoSignature(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(("Content-Type", "application/activity+json"))
            .body(
                serde_json::to_string(&serde_json::json!({
                    "error": self.kind.to_string(),
                }))
                .unwrap_or_else(|_| "{}".to_string()),
            )
    }
}

impl From<BlockingError> for ErrorKind {
    fn from(_: BlockingError) -> Self {
        ErrorKind::Canceled
    }
}

impl From<JoinError> for ErrorKind {
    fn from(_: JoinError) -> Self {
        ErrorKind::Canceled
    }
}

impl From<Infallible> for ErrorKind {
    fn from(i: Infallible) -> Self {
        match i {}
    }
}

impl From<rsa::errors::Error> for ErrorKind {
    fn from(e: rsa::errors::Error) -> Self {
        ErrorKind::Rsa(e)
    }
}
