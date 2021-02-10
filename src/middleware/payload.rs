use actix_web::{
    dev::{Payload, Service, ServiceRequest, Transform},
    http::{Method, StatusCode},
    web::BytesMut,
    HttpMessage, HttpResponse, ResponseError,
};
use futures::{
    future::{ok, LocalBoxFuture, Ready, TryFutureExt},
    stream::{once, TryStreamExt},
};
use log::{error, info};
use std::task::{Context, Poll};

#[derive(Clone, Debug)]
pub(crate) struct DebugPayload(pub bool);

#[doc(hidden)]
#[derive(Clone, Debug)]
pub(crate) struct DebugPayloadMiddleware<S>(bool, S);

#[derive(Clone, Debug, thiserror::Error)]
#[error("Failed to read payload")]
pub(crate) struct DebugError;

impl ResponseError for DebugError {
    fn status_code(&self) -> StatusCode {
        StatusCode::BAD_REQUEST
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::new(self.status_code())
    }
}

impl<S> Transform<S> for DebugPayload
where
    S: Service<Request = ServiceRequest, Error = actix_web::Error>,
    S::Future: 'static,
    S::Error: 'static,
{
    type Request = S::Request;
    type Response = S::Response;
    type Error = S::Error;
    type InitError = ();
    type Transform = DebugPayloadMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(DebugPayloadMiddleware(self.0, service))
    }
}

impl<S> Service for DebugPayloadMiddleware<S>
where
    S: Service<Request = ServiceRequest, Error = actix_web::Error>,
    S::Future: 'static,
    S::Error: 'static,
{
    type Request = S::Request;
    type Response = S::Response;
    type Error = S::Error;
    type Future = LocalBoxFuture<'static, Result<S::Response, S::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.1.poll_ready(cx)
    }

    fn call(&mut self, mut req: S::Request) -> Self::Future {
        if self.0 && req.method() == Method::POST {
            let pl = req.take_payload();
            req.set_payload(Payload::Stream(Box::pin(once(
                pl.try_fold(BytesMut::new(), |mut acc, bytes| async {
                    acc.extend(bytes);
                    Ok(acc)
                })
                .map_ok(|bytes| {
                    let bytes = bytes.freeze();
                    info!("{}", String::from_utf8_lossy(&bytes));
                    bytes
                }),
            ))));

            let fut = self.1.call(req);

            Box::pin(async move { fut.await })
        } else {
            let fut = self.1.call(req);

            Box::pin(async move { fut.await })
        }
    }
}
