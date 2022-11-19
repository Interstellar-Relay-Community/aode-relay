use dashmap::DashMap;
use indexmap::IndexMap;
use metrics::{Key, Recorder, SetRecorderError};
use metrics_util::{
    registry::{AtomicStorage, GenerationalStorage, Recency, Registry},
    MetricKindMask, Summary,
};
use quanta::Clock;
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

const SECONDS: u64 = 1;
const MINUTES: u64 = 60 * SECONDS;
const HOURS: u64 = 60 * MINUTES;
const DAYS: u64 = 24 * HOURS;

#[derive(Clone)]
pub struct MemoryCollector {
    inner: Arc<Inner>,
}

struct Inner {
    descriptions: DashMap<String, metrics::SharedString>,
    distributions: DashMap<String, IndexMap<Vec<(String, String)>, Summary>>,
    recency: Recency<Key>,
    registry: Registry<Key, GenerationalStorage<AtomicStorage>>,
}

#[derive(serde::Serialize)]
struct Counter {
    labels: Vec<(String, String)>,
    value: u64,
}

#[derive(serde::Serialize)]
struct Gauge {
    labels: Vec<(String, String)>,
    value: f64,
}

#[derive(serde::Serialize)]
struct Histogram {
    labels: Vec<(String, String)>,
    value: Vec<(f64, Option<f64>)>,
}

#[derive(serde::Serialize)]
pub(crate) struct Snapshot {
    counters: HashMap<String, Vec<Counter>>,
    gauges: HashMap<String, Vec<Gauge>>,
    histograms: HashMap<String, Vec<Histogram>>,
}

fn key_to_parts(key: &Key) -> (String, Vec<(String, String)>) {
    let labels = key
        .labels()
        .into_iter()
        .map(|label| (label.key().to_string(), label.value().to_string()))
        .collect();
    let name = key.name().to_string();
    (name, labels)
}

impl Inner {
    fn snapshot_counters(&self) -> HashMap<String, Vec<Counter>> {
        let mut counters = HashMap::new();

        for (key, counter) in self.registry.get_counter_handles() {
            let gen = counter.get_generation();
            if !self.recency.should_store_counter(&key, gen, &self.registry) {
                continue;
            }

            let (name, labels) = key_to_parts(&key);
            let value = counter.get_inner().load(Ordering::Acquire);
            counters
                .entry(name)
                .or_insert_with(Vec::new)
                .push(Counter { labels, value });
        }

        counters
    }

    fn snapshot_gauges(&self) -> HashMap<String, Vec<Gauge>> {
        let mut gauges = HashMap::new();

        for (key, gauge) in self.registry.get_gauge_handles() {
            let gen = gauge.get_generation();
            if !self.recency.should_store_gauge(&key, gen, &self.registry) {
                continue;
            }

            let (name, labels) = key_to_parts(&key);
            let value = f64::from_bits(gauge.get_inner().load(Ordering::Acquire));
            gauges
                .entry(name)
                .or_insert_with(Vec::new)
                .push(Gauge { labels, value })
        }

        gauges
    }

    fn snapshot_histograms(&self) -> HashMap<String, Vec<Histogram>> {
        for (key, histogram) in self.registry.get_histogram_handles() {
            let gen = histogram.get_generation();
            let (name, labels) = key_to_parts(&key);

            if !self
                .recency
                .should_store_histogram(&key, gen, &self.registry)
            {
                let delete_by_name = if let Some(mut by_name) = self.distributions.get_mut(&name) {
                    by_name.remove(&labels);
                    by_name.is_empty()
                } else {
                    false
                };

                if delete_by_name {
                    self.descriptions.remove(&name);
                }

                continue;
            }

            let mut outer_entry = self
                .distributions
                .entry(name.clone())
                .or_insert_with(IndexMap::new);

            let entry = outer_entry
                .entry(labels)
                .or_insert_with(Summary::with_defaults);

            histogram.get_inner().clear_with(|samples| {
                for sample in samples {
                    entry.add(*sample);
                }
            })
        }

        self.distributions
            .iter()
            .map(|entry| {
                (
                    entry.key().clone(),
                    entry
                        .value()
                        .iter()
                        .map(|(labels, summary)| Histogram {
                            labels: labels.clone(),
                            value: [0.001, 0.01, 0.05, 0.1, 0.5, 0.9, 0.99, 1.0]
                                .into_iter()
                                .map(|q| (q, summary.quantile(q)))
                                .collect(),
                        })
                        .collect(),
                )
            })
            .collect()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            counters: self.snapshot_counters(),
            gauges: self.snapshot_gauges(),
            histograms: self.snapshot_histograms(),
        }
    }
}

impl MemoryCollector {
    pub(crate) fn new() -> Self {
        MemoryCollector {
            inner: Arc::new(Inner {
                descriptions: Default::default(),
                distributions: Default::default(),
                recency: Recency::new(
                    Clock::new(),
                    MetricKindMask::ALL,
                    Some(Duration::from_secs(5 * DAYS)),
                ),
                registry: Registry::new(GenerationalStorage::atomic()),
            }),
        }
    }

    pub(crate) fn install(&self) -> Result<(), SetRecorderError> {
        metrics::set_boxed_recorder(Box::new(self.clone()))
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.inner.snapshot()
    }

    fn add_description_if_missing(
        &self,
        key: &metrics::KeyName,
        description: metrics::SharedString,
    ) {
        self.inner
            .descriptions
            .entry(key.as_str().to_owned())
            .or_insert(description);
    }
}

impl Recorder for MemoryCollector {
    fn describe_counter(
        &self,
        key: metrics::KeyName,
        _: Option<metrics::Unit>,
        description: metrics::SharedString,
    ) {
        self.add_description_if_missing(&key, description)
    }

    fn describe_gauge(
        &self,
        key: metrics::KeyName,
        _: Option<metrics::Unit>,
        description: metrics::SharedString,
    ) {
        self.add_description_if_missing(&key, description)
    }

    fn describe_histogram(
        &self,
        key: metrics::KeyName,
        _: Option<metrics::Unit>,
        description: metrics::SharedString,
    ) {
        self.add_description_if_missing(&key, description)
    }

    fn register_counter(&self, key: &Key) -> metrics::Counter {
        self.inner
            .registry
            .get_or_create_counter(key, |c| c.clone().into())
    }

    fn register_gauge(&self, key: &Key) -> metrics::Gauge {
        self.inner
            .registry
            .get_or_create_gauge(key, |c| c.clone().into())
    }

    fn register_histogram(&self, key: &Key) -> metrics::Histogram {
        self.inner
            .registry
            .get_or_create_histogram(key, |c| c.clone().into())
    }
}

/*
struct Bucket {
    begin: Instant,
    summary: Summary,
}

pub(crate) struct RollingSummary {
    buckets: Vec<Bucket>,
    bucket_duration: Duration,
    expire_after: Duration,
    count: usize,
}

impl Default for RollingSummary {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(5 * MINUTES),
            Duration::from_secs(1 * DAYS),
        )
    }
}

impl RollingSummary {
    fn new(bucket_duration: Duration, expire_after: Duration) -> Self {
        Self {
            buckets: Vec::new(),
            bucket_duration,
            expire_after,
            count: 0,
        }
    }

    fn add(&mut self, value: f64, now: Instant) {
        self.count += 1;

        // try adding to existing bucket
        for bucket in &mut self.buckets {
            let end = bucket.begin + self.bucket_duration;

            if now >= end {
                break;
            }

            if now >= bucket.begin {
                bucket.summary.add(value);
                return;
            }
        }

        // if we're adding a new bucket, clean old buckets first
        if let Some(cutoff) = now.checked_sub(self.expire_after) {
            self.buckets.retain(|b| b.begin > cutoff);
        }

        let mut summary = Summary::with_defaults();
        summary.add(value);

        // if there's no buckets, make one and return
        if self.buckets.is_empty() {
            self.buckets.push(Bucket {
                summary,
                begin: now,
            });
            return;
        }

        let mut begin = self.buckets[0].begin;

        // there are buckets, but none can hold our value, see why
        if now < self.buckets[0].begin {
            // create an old bucket

            while now < begin {
                begin -= self.bucket_duration;
            }

            self.buckets.push(Bucket { begin, summary });
            self.buckets.sort_unstable_by(|a, b| b.begin.cmp(&a.begin));
        } else {
            // create a new bucket
            let mut end = self.buckets[0].begin + self.bucket_duration;

            while now >= end {
                begin += self.bucket_duration;
                end += self.bucket_duration;
            }

            self.buckets.insert(0, Bucket { begin, summary });
        }
    }

    fn snapshot(&self, now: Instant) -> Summary {
        let cutoff = now.checked_sub(self.expire_after);
        let mut acc = Summary::with_defaults();

        let summaries = self
            .buckets
            .iter()
            .filter(|b| cutoff.map(|c| b.begin > c).unwrap_or(true))
            .map(|b| &b.summary);

        for item in summaries {
            acc.merge(item)
                .expect("All summaries are created with default settings");
        }

        acc
    }
}
*/
