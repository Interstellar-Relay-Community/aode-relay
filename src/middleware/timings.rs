use actix_web::{
    body::MessageBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http::StatusCode,
};
use std::{
    future::{ready, Future, Ready},
    time::Instant,
};

pub(crate) struct Timings;
pub(crate) struct TimingsMiddleware<S>(S);

struct LogOnDrop {
    begin: Instant,
    path: String,
    method: String,
    arm: bool,
}

pin_project_lite::pin_project! {
    pub(crate) struct TimingsFuture<F> {
        #[pin]
        future: F,

        log_on_drop: Option<LogOnDrop>,
    }
}

pin_project_lite::pin_project! {
    pub(crate) struct TimingsBody<B> {
        #[pin]
        body: B,

        log_on_drop: LogOnDrop,
    }
}

impl Drop for LogOnDrop {
    fn drop(&mut self) {
        if self.arm {
            let duration = self.begin.elapsed();
            metrics::histogram!("relay.request.complete", duration, "path" => self.path.clone(), "method" => self.method.clone());
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for Timings
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
{
    type Response = ServiceResponse<TimingsBody<B>>;
    type Error = S::Error;
    type InitError = ();
    type Transform = TimingsMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(TimingsMiddleware(service)))
    }
}

impl<S, B> Service<ServiceRequest> for TimingsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
{
    type Response = ServiceResponse<TimingsBody<B>>;
    type Error = S::Error;
    type Future = TimingsFuture<S::Future>;

    fn poll_ready(
        &self,
        ctx: &mut core::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let log_on_drop = LogOnDrop {
            begin: Instant::now(),
            path: req.path().to_string(),
            method: req.method().to_string(),
            arm: false,
        };

        let future = self.0.call(req);

        TimingsFuture {
            future,
            log_on_drop: Some(log_on_drop),
        }
    }
}

impl<F, B> Future for TimingsFuture<F>
where
    F: Future<Output = Result<ServiceResponse<B>, actix_web::Error>>,
{
    type Output = Result<ServiceResponse<TimingsBody<B>>, actix_web::Error>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();

        let res = std::task::ready!(this.future.poll(cx));

        let mut log_on_drop = this
            .log_on_drop
            .take()
            .expect("TimingsFuture polled after completion");

        let status = match &res {
            Ok(res) => res.status(),
            Err(e) => e.as_response_error().status_code(),
        };

        log_on_drop.arm =
            status != StatusCode::NOT_FOUND && status != StatusCode::METHOD_NOT_ALLOWED;

        let res = res.map(|r| r.map_body(|_, body| TimingsBody { body, log_on_drop }));

        std::task::Poll::Ready(res)
    }
}

impl<B: MessageBody> MessageBody for TimingsBody<B> {
    type Error = B::Error;

    fn size(&self) -> actix_web::body::BodySize {
        self.body.size()
    }

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<actix_web::web::Bytes, Self::Error>>> {
        self.project().body.poll_next(cx)
    }
}
