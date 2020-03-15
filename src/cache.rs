use std::collections::{BTreeMap, HashMap, LinkedList};

pub struct WeightedCache<K, V>
where
    K: std::hash::Hash + Eq + Clone,
{
    size: usize,
    capacity: usize,
    forward: HashMap<K, (V, Count)>,
    backward: BTreeMap<Count, LinkedList<K>>,
}

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Count(usize);

impl<K, V> WeightedCache<K, V>
where
    K: std::hash::Hash + Eq + Clone,
{
    /// Create a new Weighted Cache
    ///
    /// panics if capacity is 0
    pub fn new(capacity: usize) -> Self {
        if capacity == 0 {
            panic!("Cache Capacity must be > 0");
        }

        WeightedCache {
            size: 0,
            capacity,
            forward: HashMap::new(),
            backward: BTreeMap::new(),
        }
    }

    /// Gets a value from the weighted cache
    pub fn get(&mut self, key: K) -> Option<&V> {
        let (value, count) = self.forward.get_mut(&key)?;

        if let Some(v) = self.backward.get_mut(count) {
            v.drain_filter(|item| item == &key);
        }

        count.0 += 1;

        let entry = self.backward.entry(*count).or_insert(LinkedList::new());
        entry.push_back(key);

        Some(&*value)
    }

    /// set a value in the weighted cache
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if self.forward.contains_key(&key) {
            return None;
        }

        let ret = if self.size >= self.capacity {
            self.remove_least()
        } else {
            None
        };

        let count = Count(1);
        self.forward.insert(key.clone(), (value, count));
        let entry = self.backward.entry(count).or_insert(LinkedList::new());

        entry.push_back(key);
        self.size += 1;

        ret
    }

    fn remove_least(&mut self) -> Option<V> {
        let items = self.backward.values_mut().next()?;

        let oldest = items.pop_front()?;
        let length = items.len();
        drop(items);

        let (item, count) = self.forward.remove(&oldest)?;

        if length == 0 {
            self.backward.remove(&count);
            self.backward = self
                .backward
                .clone()
                .into_iter()
                .map(|(mut k, v)| {
                    k.0 -= count.0;
                    (k, v)
                })
                .collect();
        }

        self.size -= 1;

        Some(item)
    }
}
