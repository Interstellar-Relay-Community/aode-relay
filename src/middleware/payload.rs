use actix_web::{
    dev::{Payload, Service, ServiceRequest, Transform},
    http::StatusCode,
    HttpMessage, HttpResponse, ResponseError,
};
use bytes::BytesMut;
use futures::{
    future::{ok, LocalBoxFuture, Ready},
    stream::StreamExt,
};
use log::info;
use std::task::{Context, Poll};
use tokio::sync::mpsc::channel;

#[derive(Clone, Debug)]
pub struct DebugPayload(pub bool);

#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct DebugPayloadMiddleware<S>(bool, S);

#[derive(Clone, Debug, thiserror::Error)]
#[error("Failed to read payload")]
pub struct DebugError;

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
        if self.0 {
            let (mut tx, rx) = channel(1);

            let mut pl = req.take_payload();
            req.set_payload(Payload::Stream(Box::pin(rx)));

            let fut = self.1.call(req);

            return Box::pin(async move {
                let mut bytes = BytesMut::new();

                while let Some(res) = pl.next().await {
                    let b = res.map_err(|_| DebugError)?;
                    bytes.extend(b);
                }

                info!("{}", String::from_utf8_lossy(bytes.as_ref()));

                tx.send(Ok(bytes.freeze())).await.map_err(|_| DebugError)?;

                fut.await
            });
        }

        let fut = self.1.call(req);

        Box::pin(async move { fut.await })
    }
}
