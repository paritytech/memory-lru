// Copyright (c) 2015-2021 Parity Technologies

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! A memory-based LRU cache.

use lru::LruCache;

use std::hash::Hash;
use std::num::NonZeroUsize;

const INITIAL_CAPACITY: Option<NonZeroUsize> = NonZeroUsize::new(4);

/// An indicator of the resident in memory of a value.
pub trait ResidentSize {
    /// Return the resident size of the value. Users of the trait will depend
    /// on this value to remain stable unless the value is mutated.
    fn resident_size(&self) -> usize;
}

/// An LRU-cache which operates on memory used.
pub struct MemoryLruCache<K, V> {
    inner: LruCache<K, V>,
    cur_size: usize,
    max_size: usize,
}

impl<K: Eq + Hash, V: ResidentSize> MemoryLruCache<K, V> {
    /// Create a new cache with a maximum cumulative size of values.
    pub fn new(max_size: usize) -> Self {
        MemoryLruCache {
            inner: LruCache::new(INITIAL_CAPACITY.expect("4 != 0; qed")),
            max_size: max_size,
            cur_size: 0,
        }
    }

    /// Insert an item.
    pub fn insert(&mut self, key: K, val: V) {
        let cap = self.inner.cap().get();

        // grow the cache as necessary; it operates on amount of items
        // but we're working based on memory usage.
        if self.inner.len() == cap {
            let next_cap = NonZeroUsize::new(cap.saturating_mul(2)).expect(
                "only returns None if value is zero; cap is guaranteed to be non-zero; qed",
            );
            self.inner.resize(next_cap);
        }

        self.cur_size += val.resident_size();

        // account for any element displaced from the cache.
        if let Some(lru) = self.inner.put(key, val) {
            self.cur_size -= lru.resident_size();
        }

        self.readjust_down();
    }

    /// Get a reference to an item in the cache. It is a logic error for its
    /// heap size to be altered while borrowed.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.inner.get(key)
    }

    /// Execute a closure with the value under the provided key.
    pub fn with_mut<U>(&mut self, key: &K, with: impl FnOnce(Option<&mut V>) -> U) -> U {
        let mut val = self.inner.get_mut(key);
        let prev_size = val.as_ref().map_or(0, |v| v.resident_size());

        let res = with(val.as_mut().map(|v: &mut &mut V| &mut **v));

        let new_size = val.as_ref().map_or(0, |v| v.resident_size());

        self.cur_size -= prev_size;
        self.cur_size += new_size;

        self.readjust_down();

        res
    }

    /// Currently-used size of values in bytes.
    pub fn current_size(&self) -> usize {
        self.cur_size
    }

    /// Returns the number of key-value pairs that are currently in the cache.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns a bool indicating whether the given key is in the cache.
    /// Does not update the LRU list.
    pub fn contains(&self, key: &K) -> bool {
        self.inner.contains(key)
    }

    /// Returns a bool indicating whether the cache is empty or not.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns a reference to the value corresponding to the key in the cache or
    /// None if it is not present in the cache. Unlike get, peek does not update the
    /// LRU list so the key's position will be unchanged.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.inner.peek(key)
    }

    fn readjust_down(&mut self) {
        // remove elements until we are below the memory target.
        while self.cur_size > self.max_size {
            match self.inner.pop_lru() {
                Some((_, v)) => self.cur_size -= v.resident_size(),
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ResidentSize for Vec<u8> {
        fn resident_size(&self) -> usize {
            self.len()
        }
    }

    #[test]
    fn it_works() {
        let mut cache = MemoryLruCache::new(256);
        let val1 = vec![0u8; 100];
        let size1 = val1.resident_size();
        assert_eq!(cache.len(), 0);
        cache.insert("hello", val1);

        assert_eq!(cache.current_size(), size1);

        let val2 = vec![0u8; 210];
        let size2 = val2.resident_size();
        cache.insert("world", val2);

        assert!(cache.get(&"hello").is_none());
        assert!(cache.get(&"world").is_some());

        assert_eq!(cache.current_size(), size2);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn it_works_if_cur_size_equals_max_size() {
        let mut cache = MemoryLruCache::new(8);
        cache.insert(1, vec![0u8, 1u8]);
        cache.insert(2, vec![2u8, 3u8]);
        cache.insert(3, vec![4u8, 5u8]);
        cache.insert(4, vec![6u8, 7u8]);
        cache.insert(5, vec![8u8, 9u8]);

        assert_eq!(Some(&vec![2u8, 3u8]), cache.get(&2));
    }
}
