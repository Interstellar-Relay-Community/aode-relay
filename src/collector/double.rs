use metrics::{CounterFn, GaugeFn, HistogramFn, Key, Recorder, SetRecorderError};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct DoubleRecorder<R, S> {
    first: R,
    second: S,
}

struct DoubleCounter {
    first: metrics::Counter,
    second: metrics::Counter,
}

struct DoubleGauge {
    first: metrics::Gauge,
    second: metrics::Gauge,
}

struct DoubleHistogram {
    first: metrics::Histogram,
    second: metrics::Histogram,
}

impl<R, S> DoubleRecorder<R, S> {
    pub(crate) fn new(first: R, second: S) -> Self {
        DoubleRecorder { first, second }
    }

    pub(crate) fn install(self) -> Result<(), SetRecorderError>
    where
        R: Recorder + 'static,
        S: Recorder + 'static,
    {
        metrics::set_boxed_recorder(Box::new(self))
    }
}

impl<R, S> Recorder for DoubleRecorder<R, S>
where
    R: Recorder,
    S: Recorder,
{
    fn describe_counter(
        &self,
        key: metrics::KeyName,
        unit: Option<metrics::Unit>,
        description: metrics::SharedString,
    ) {
        self.first
            .describe_counter(key.clone(), unit, description.clone());
        self.second.describe_counter(key, unit, description);
    }

    fn describe_gauge(
        &self,
        key: metrics::KeyName,
        unit: Option<metrics::Unit>,
        description: metrics::SharedString,
    ) {
        self.first
            .describe_gauge(key.clone(), unit, description.clone());
        self.second.describe_gauge(key, unit, description);
    }

    fn describe_histogram(
        &self,
        key: metrics::KeyName,
        unit: Option<metrics::Unit>,
        description: metrics::SharedString,
    ) {
        self.first
            .describe_histogram(key.clone(), unit, description.clone());
        self.second.describe_histogram(key, unit, description);
    }

    fn register_counter(&self, key: &Key) -> metrics::Counter {
        let first = self.first.register_counter(key);
        let second = self.second.register_counter(key);

        metrics::Counter::from_arc(Arc::new(DoubleCounter { first, second }))
    }

    fn register_gauge(&self, key: &Key) -> metrics::Gauge {
        let first = self.first.register_gauge(key);
        let second = self.second.register_gauge(key);

        metrics::Gauge::from_arc(Arc::new(DoubleGauge { first, second }))
    }

    fn register_histogram(&self, key: &Key) -> metrics::Histogram {
        let first = self.first.register_histogram(key);
        let second = self.second.register_histogram(key);

        metrics::Histogram::from_arc(Arc::new(DoubleHistogram { first, second }))
    }
}

impl CounterFn for DoubleCounter {
    fn increment(&self, value: u64) {
        self.first.increment(value);
        self.second.increment(value);
    }

    fn absolute(&self, value: u64) {
        self.first.absolute(value);
        self.second.absolute(value);
    }
}

impl GaugeFn for DoubleGauge {
    fn increment(&self, value: f64) {
        self.first.increment(value);
        self.second.increment(value);
    }

    fn decrement(&self, value: f64) {
        self.first.decrement(value);
        self.second.decrement(value);
    }

    fn set(&self, value: f64) {
        self.first.set(value);
        self.second.set(value);
    }
}

impl HistogramFn for DoubleHistogram {
    fn record(&self, value: f64) {
        self.first.record(value);
        self.second.record(value);
    }
}
