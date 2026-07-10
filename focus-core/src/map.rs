use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

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
}

impl<K: MapKey, V> Index<K> for Map<K, V> {
    type Output = V;

    fn index(&self, key: K) -> &V {
        self.values
            .get(key.index())
            .unwrap_or_else(|| panic!("key {:?} is missing", key))
    }
}

impl<K: MapKey, V> IndexMut<K> for Map<K, V> {
    fn index_mut(&mut self, key: K) -> &mut V {
        let len = self.values.len();
        self.values
            .get_mut(key.index())
            .unwrap_or_else(|| panic!("key {:?} is missing from map of len {}", key, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    struct TestId(usize);

    impl MapKey for TestId {
        fn index(self) -> usize {
            self.0
        }

        fn from_index(index: usize) -> Self {
            TestId(index)
        }
    }

    #[test]
    fn insert_index_and_keys_are_append_only() {
        let mut map = Map::new();
        map.insert(TestId(0), "a");
        map.insert(TestId(1), "b");

        assert_eq!(map[TestId(0)], "a");
        assert_eq!(map[TestId(1)], "b");
        assert_eq!(map.keys().map(|id| id.0).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn index_mut_updates_value() {
        let mut map = Map::new();
        map.insert(TestId(0), "a");
        map[TestId(0)] = "b";

        assert_eq!(map[TestId(0)], "b");
    }

    #[test]
    #[should_panic(expected = "already present")]
    fn insert_panics_if_key_exists() {
        let mut map = Map::new();
        map.insert(TestId(0), "a");
        map.insert(TestId(0), "b");
    }

    #[test]
    #[should_panic(expected = "would create a hole")]
    fn insert_panics_if_key_skips_index() {
        let mut map = Map::new();
        map.insert(TestId(1), "b");
    }

    #[test]
    #[should_panic(expected = "missing")]
    fn index_panics_if_key_missing() {
        let map: Map<TestId, &str> = Map::new();
        let _ = map[TestId(0)];
    }
}
