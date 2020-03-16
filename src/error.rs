use activitystreams::primitives::XsdAnyUriError;
use actix_web::{error::ResponseError, http::StatusCode, HttpResponse};
use log::error;
use rsa_pem::KeyError;
use std::{convert::Infallible, io::Error};

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

    #[error("Couldn't decode base64")]
    Base64(#[from] base64::DecodeError),

    #[error("Invalid algorithm provided to verifier")]
    Algorithm,

    #[error("Wrong ActivityPub kind")]
    Kind,

    #[error("Object has already been relayed")]
    Duplicate,

    #[error("Actor is blocked")]
    Blocked,

    #[error("Actor is not whitelisted")]
    Whitelist,

    #[error("Couldn't send request")]
    SendRequest,

    #[error("Couldn't receive request response")]
    ReceiveResponse,

    #[error("Response has invalid status code")]
    Status,

    #[error("URI is missing domain field")]
    Domain,

    #[error("Public key is missing")]
    MissingKey,
}

impl ResponseError for MyError {
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError()
            .header("Content-Type", "application/activity+json")
            .json(serde_json::json!({}))
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
