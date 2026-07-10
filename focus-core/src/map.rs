use std::fmt::Debug;
use std::marker::PhantomData;

pub(crate) trait MapKey: Copy + Debug {
    fn index(self) -> usize;
    fn from_index(index: usize) -> Self;
}

pub(crate) struct Map<K, V> {
    values: Vec<V>,
    _key: PhantomData<fn(K)>,
}

impl<K, V> Default for Map<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Map<K, V> {
    pub(crate) fn new() -> Self {
        Map {
            values: Vec::new(),
            _key: PhantomData,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.values.iter()
    }
}

impl<K: MapKey, V> Map<K, V> {
    pub(crate) fn keys(&self) -> impl Iterator<Item = K> + '_ {
        (0..self.values.len()).map(K::from_index)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (K, &V)> + '_ {
        self.values
            .iter()
            .enumerate()
            .map(|(index, value)| (K::from_index(index), value))
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        let index = key.index();
        if index < self.values.len() {
            panic!("key {:?} is already present", key);
        }
        if index > self.values.len() {
            panic!(
                "key {:?} would create a hole in map of len {}",
                key,
                self.values.len()
            );
        }
        self.values.push(value);
    }

    pub(crate) fn get(&self, key: K) -> &V {
        self.values
            .get(key.index())
            .unwrap_or_else(|| panic!("key {:?} is missing", key))
    }

    pub(crate) fn get_mut(&mut self, key: K) -> &mut V {
        let len = self.values.len();
        self.values
            .get_mut(key.index())
            .unwrap_or_else(|| panic!("key {:?} is missing from map of len {}", key, len))
    }
}
