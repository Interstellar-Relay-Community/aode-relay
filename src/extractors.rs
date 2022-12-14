use actix_web::{
    dev::Payload,
    error::{BlockingError, ParseError},
    http::{
        header::{from_one_raw_str, Header, HeaderName, HeaderValue, TryIntoHeaderValue},
        StatusCode,
    },
    web::Data,
    FromRequest, HttpMessage, HttpRequest, HttpResponse, ResponseError,
};
use bcrypt::{BcryptError, DEFAULT_COST};
use futures_util::future::LocalBoxFuture;
use http_signature_normalization_actix::prelude::InvalidHeaderValue;
use std::{convert::Infallible, str::FromStr, time::Instant};
use tracing_error::SpanTrace;

use crate::db::Db;

#[derive(Clone)]
pub(crate) struct AdminConfig {
    hashed_api_token: String,
}

impl AdminConfig {
    pub(crate) fn build(api_token: &str) -> Result<Self, Error> {
        Ok(AdminConfig {
            hashed_api_token: bcrypt::hash(api_token, DEFAULT_COST).map_err(Error::bcrypt_hash)?,
        })
    }

    fn verify(&self, token: XApiToken) -> Result<bool, Error> {
        bcrypt::verify(&token.0, &self.hashed_api_token).map_err(Error::bcrypt_verify)
    }
}

pub(crate) struct Admin {
    db: Data<Db>,
}

impl Admin {
    fn prepare_verify(
        req: &HttpRequest,
    ) -> Result<(Data<Db>, Data<AdminConfig>, XApiToken), Error> {
        let hashed_api_token = req
            .app_data::<Data<AdminConfig>>()
            .ok_or_else(Error::missing_config)?
            .clone();

        let x_api_token = XApiToken::parse(req).map_err(Error::parse_header)?;

        let db = req
            .app_data::<Data<Db>>()
            .ok_or_else(Error::missing_db)?
            .clone();

        Ok((db, hashed_api_token, x_api_token))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn verify(
        hashed_api_token: Data<AdminConfig>,
        x_api_token: XApiToken,
    ) -> Result<(), Error> {
        let span = tracing::Span::current();
        if actix_web::web::block(move || span.in_scope(|| hashed_api_token.verify(x_api_token)))
            .await
            .map_err(Error::canceled)??
        {
            return Ok(());
        }

        Err(Error::invalid())
    }

    pub(crate) fn db_ref(&self) -> &Db {
        &self.db
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed authentication")]
pub(crate) struct Error {
    context: String,
    #[source]
    kind: ErrorKind,
}

impl Error {
    fn invalid() -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: ErrorKind::Invalid,
        }
    }

    fn missing_config() -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: ErrorKind::MissingConfig,
        }
    }

    fn missing_db() -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: ErrorKind::MissingDb,
        }
    }

    fn bcrypt_verify(e: BcryptError) -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: ErrorKind::BCryptVerify(e),
        }
    }

    fn bcrypt_hash(e: BcryptError) -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: ErrorKind::BCryptHash(e),
        }
    }

    fn parse_header(e: ParseError) -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: ErrorKind::ParseHeader(e),
        }
    }

    fn canceled(_: BlockingError) -> Self {
        Error {
            context: SpanTrace::capture().to_string(),
            kind: ErrorKind::Canceled,
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ErrorKind {
    #[error("Invalid API Token")]
    Invalid,

    #[error("Missing Config")]
    MissingConfig,

    #[error("Missing Db")]
    MissingDb,

    #[error("Panic in verify")]
    Canceled,

    #[error("Verifying")]
    BCryptVerify(#[source] BcryptError),

    #[error("Hashing")]
    BCryptHash(#[source] BcryptError),

    #[error("Parse Header")]
    ParseHeader(#[source] ParseError),
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match self.kind {
            ErrorKind::Invalid | ErrorKind::ParseHeader(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .json(serde_json::json!({ "msg": self.kind.to_string() }))
    }
}

impl FromRequest for Admin {
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let now = Instant::now();
        let res = Self::prepare_verify(req);
        Box::pin(async move {
            let (db, c, t) = res?;
            Self::verify(c, t).await?;
            metrics::histogram!(
                "relay.admin.verify",
                now.elapsed().as_micros() as f64 / 1_000_000_f64
            );
            Ok(Admin { db })
        })
    }
}

pub(crate) struct XApiToken(String);

impl XApiToken {
    pub(crate) fn new(token: String) -> Self {
        Self(token)
    }
}

impl Header for XApiToken {
    fn name() -> HeaderName {
        HeaderName::from_static("x-api-token")
    }

    fn parse<M: HttpMessage>(msg: &M) -> Result<Self, ParseError> {
        from_one_raw_str(msg.headers().get(Self::name()))
    }
}

impl TryIntoHeaderValue for XApiToken {
    type Error = InvalidHeaderValue;

    fn try_into_value(self) -> Result<HeaderValue, Self::Error> {
        HeaderValue::from_str(&self.0)
    }
}

impl FromStr for XApiToken {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(XApiToken(s.to_string()))
    }
}
