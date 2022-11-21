use actix_web::{
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::StatusCode,
};
use futures_util::future::LocalBoxFuture;
use std::{
    future::{ready, Ready},
    time::Instant,
};

pub(crate) struct Timings;
pub(crate) struct TimingsMiddleware<S>(S);

struct LogOnDrop {
    begin: Instant,
    path: String,
    method: String,
    disarm: bool,
}

impl Drop for LogOnDrop {
    fn drop(&mut self) {
        if !self.disarm {
            let duration = self.begin.elapsed();
            metrics::histogram!("relay.request.complete", duration, "path" => self.path.clone(), "method" => self.method.clone());
        }
    }
}

impl<S> Transform<S, ServiceRequest> for Timings
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = actix_web::Error>,
    S::Future: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type InitError = ();
    type Transform = TimingsMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(TimingsMiddleware(service)))
    }
}

impl<S> Service<ServiceRequest> for TimingsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = actix_web::Error>,
    S::Future: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = LocalBoxFuture<'static, Result<S::Response, S::Error>>;

    fn poll_ready(
        &self,
        ctx: &mut core::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let mut logger = LogOnDrop {
            begin: Instant::now(),
            path: req.path().to_string(),
            method: req.method().to_string(),
            disarm: false,
        };
        let fut = self.0.call(req);

        Box::pin(async move {
            let res = fut.await;

            let status = match &res {
                Ok(res) => res.status(),
                Err(e) => e.as_response_error().status_code(),
            };
            if status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED {
                logger.disarm = true;
            }

            // TODO: Drop after body write
            drop(logger);

            res
        })
    }
}
