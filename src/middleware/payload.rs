use actix_web::{
    dev::{Payload, Service, ServiceRequest, Transform},
    http::Method,
    web::BytesMut,
    HttpMessage,
};
use futures_util::{
    future::TryFutureExt,
    stream::{once, TryStreamExt},
};
use std::{
    future::{ready, Ready},
    task::{Context, Poll},
};

#[derive(Clone, Debug)]
pub(crate) struct DebugPayload(pub bool);

#[doc(hidden)]
#[derive(Clone, Debug)]
pub(crate) struct DebugPayloadMiddleware<S>(bool, S);

impl<S> Transform<S, ServiceRequest> for DebugPayload
where
    S: Service<ServiceRequest, Error = actix_web::Error>,
    S::Future: 'static,
    S::Error: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type InitError = ();
    type Transform = DebugPayloadMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(DebugPayloadMiddleware(self.0, service)))
    }
}

impl<S> Service<ServiceRequest> for DebugPayloadMiddleware<S>
where
    S: Service<ServiceRequest, Error = actix_web::Error>,
    S::Future: 'static,
    S::Error: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.1.poll_ready(cx)
    }

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        if self.0 && req.method() == Method::POST {
            let pl = req.take_payload();
            req.set_payload(Payload::Stream {
                payload: Box::pin(once(
                    pl.try_fold(BytesMut::new(), |mut acc, bytes| async {
                        acc.extend(bytes);
                        Ok(acc)
                    })
                    .map_ok(|bytes| {
                        let bytes = bytes.freeze();
                        tracing::info!("{}", String::from_utf8_lossy(&bytes));
                        bytes
                    }),
                )),
            });

            self.1.call(req)
        } else {
            self.1.call(req)
        }
    }
}
