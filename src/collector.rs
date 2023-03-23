use metrics::{Key, Recorder, SetRecorderError};
use metrics_util::{
    registry::{AtomicStorage, GenerationalStorage, Recency, Registry},
    MetricKindMask, Summary,
};
use quanta::Clock;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{atomic::Ordering, Arc, RwLock},
    time::Duration,
};

const SECONDS: u64 = 1;
const MINUTES: u64 = 60 * SECONDS;
const HOURS: u64 = 60 * MINUTES;
const DAYS: u64 = 24 * HOURS;

type DistributionMap = BTreeMap<Vec<(String, String)>, Summary>;

#[derive(Clone)]
pub struct MemoryCollector {
    inner: Arc<Inner>,
}

struct Inner {
    descriptions: RwLock<HashMap<String, metrics::SharedString>>,
    distributions: RwLock<HashMap<String, DistributionMap>>,
    recency: Recency<Key>,
    registry: Registry<Key, GenerationalStorage<AtomicStorage>>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Counter {
    labels: BTreeMap<String, String>,
    value: u64,
}

impl std::fmt::Display for Counter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let labels = self
            .labels
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ");

        write!(f, "{labels} - {}", self.value)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Gauge {
    labels: BTreeMap<String, String>,
    value: f64,
}

impl std::fmt::Display for Gauge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let labels = self
            .labels
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ");

        write!(f, "{labels} - {}", self.value)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Histogram {
    labels: BTreeMap<String, String>,
    value: Vec<(f64, Option<f64>)>,
}

impl std::fmt::Display for Histogram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let labels = self
            .labels
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ");

        let value = self
            .value
            .iter()
            .map(|(k, v)| {
                if let Some(v) = v {
                    format!("{k}: {v:.6}")
                } else {
                    format!("{k}: None,")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        write!(f, "{labels} - {value}")
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Snapshot {
    counters: HashMap<String, Vec<Counter>>,
    gauges: HashMap<String, Vec<Gauge>>,
    histograms: HashMap<String, Vec<Histogram>>,
}

const PAIRS: [((&str, &str), &str); 2] = [
    (
        (
            "background-jobs.worker.started",
            "background-jobs.worker.finished",
        ),
        "background-jobs.worker.running",
    ),
    (
        (
            "background-jobs.job.started",
            "background-jobs.job.finished",
        ),
        "background-jobs.job.running",
    ),
];

#[derive(Default)]
struct MergeCounter {
    start: Option<Counter>,
    finish: Option<Counter>,
}

impl MergeCounter {
    fn merge(self) -> Option<Counter> {
        match (self.start, self.finish) {
            (Some(start), Some(end)) => Some(Counter {
                labels: start.labels,
                value: start.value.saturating_sub(end.value),
            }),
            (Some(only), None) => Some(only),
            (None, Some(only)) => Some(Counter {
                labels: only.labels,
                value: 0,
            }),
            (None, None) => None,
        }
    }
}

impl Snapshot {
    pub(crate) fn present(self) {
        if !self.counters.is_empty() {
            println!("Counters");
            let mut merging = HashMap::new();
            for (key, counters) in self.counters {
                if let Some(((start, _), name)) = PAIRS
                    .iter()
                    .find(|((start, finish), _)| *start == key || *finish == key)
                {
                    let entry = merging.entry(name).or_insert_with(HashMap::new);

                    for counter in counters {
                        let mut merge_counter = entry
                            .entry(counter.labels.clone())
                            .or_insert_with(MergeCounter::default);
                        if key == *start {
                            merge_counter.start = Some(counter);
                        } else {
                            merge_counter.finish = Some(counter);
                        }
                    }

                    continue;
                }

                println!("\t{key}");
                for counter in counters {
                    println!("\t\t{counter}");
                }
            }

            for (key, counters) in merging {
                println!("\t{key}");

                for (_, counter) in counters {
                    if let Some(counter) = counter.merge() {
                        println!("\t\t{counter}");
                    }
                }
            }
        }

        if !self.gauges.is_empty() {
            println!("Gauges");
            for (key, gauges) in self.gauges {
                println!("\t{key}");

                for gauge in gauges {
                    println!("\t\t{gauge}");
                }
            }
        }

        if !self.histograms.is_empty() {
            println!("Histograms");
            for (key, histograms) in self.histograms {
                println!("\t{key}");

                for histogram in histograms {
                    println!("\t\t{histogram}");
                }
            }
        }
    }
}

fn key_to_parts(key: &Key) -> (String, Vec<(String, String)>) {
    let labels = key
        .labels()
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
            counters.entry(name).or_insert_with(Vec::new).push(Counter {
                labels: labels.into_iter().collect(),
                value,
            });
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
            gauges.entry(name).or_insert_with(Vec::new).push(Gauge {
                labels: labels.into_iter().collect(),
                value,
            })
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
                let mut d = self.distributions.write().unwrap();
                let delete_by_name = if let Some(by_name) = d.get_mut(&name) {
                    by_name.remove(&labels);
                    by_name.is_empty()
                } else {
                    false
                };
                drop(d);

                if delete_by_name {
                    self.descriptions.write().unwrap().remove(&name);
                }

                continue;
            }

            let mut d = self.distributions.write().unwrap();
            let outer_entry = d.entry(name.clone()).or_insert_with(BTreeMap::new);

            let entry = outer_entry
                .entry(labels)
                .or_insert_with(Summary::with_defaults);

            histogram.get_inner().clear_with(|samples| {
                for sample in samples {
                    entry.add(*sample);
                }
            })
        }

        let d = self.distributions.read().unwrap().clone();
        d.into_iter()
            .map(|(key, value)| {
                (
                    key,
                    value
                        .into_iter()
                        .map(|(labels, summary)| Histogram {
                            labels: labels.into_iter().collect(),
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

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.inner.snapshot()
    }

    fn add_description_if_missing(
        &self,
        key: &metrics::KeyName,
        description: metrics::SharedString,
    ) {
        let mut d = self.inner.descriptions.write().unwrap();
        d.entry(key.as_str().to_owned()).or_insert(description);
    }

    pub(crate) fn install(&self) -> Result<(), SetRecorderError> {
        metrics::set_boxed_recorder(Box::new(self.clone()))
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
