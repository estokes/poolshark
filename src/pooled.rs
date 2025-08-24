use super::Poolable;
#[cfg(feature = "indexmap")]
use indexmap::{IndexMap, IndexSet};
use std::{
    cmp::Eq,
    collections::{HashMap, HashSet, VecDeque},
    default::Default,
    hash::{BuildHasher, Hash},
};

macro_rules! impl_hashmap {
    ($ty:ident) => {
        impl<K, V, R> Poolable for $ty<K, V, R>
        where
            K: Hash + Eq,
            R: Default + BuildHasher,
        {
            fn empty() -> Self {
                $ty::default()
            }

            fn reset(&mut self) {
                self.clear()
            }

            fn capacity(&self) -> usize {
                $ty::capacity(self)
            }
        }
    };
}

impl_hashmap!(HashMap);
#[cfg(feature = "indexmap")]
impl_hashmap!(IndexMap);

macro_rules! impl_hashset {
    ($ty:ident) => {
        impl<K, R> Poolable for $ty<K, R>
        where
            K: Hash + Eq,
            R: Default + BuildHasher,
        {
            fn empty() -> Self {
                $ty::default()
            }

            fn reset(&mut self) {
                self.clear()
            }

            fn capacity(&self) -> usize {
                $ty::capacity(self)
            }
        }
    };
}

impl_hashset!(HashSet);
#[cfg(feature = "indexmap")]
impl_hashset!(IndexSet);

impl<T> Poolable for Vec<T> {
    fn empty() -> Self {
        Vec::new()
    }

    fn reset(&mut self) {
        self.clear()
    }

    fn capacity(&self) -> usize {
        Vec::capacity(self)
    }
}

impl<T> Poolable for VecDeque<T> {
    fn empty() -> Self {
        VecDeque::new()
    }

    fn reset(&mut self) {
        self.clear()
    }

    fn capacity(&self) -> usize {
        VecDeque::capacity(self)
    }
}

impl Poolable for String {
    fn empty() -> Self {
        String::new()
    }

    fn reset(&mut self) {
        self.clear()
    }

    fn capacity(&self) -> usize {
        self.capacity()
    }
}

impl<T: Poolable> Poolable for Option<T> {
    fn empty() -> Self {
        None
    }

    fn reset(&mut self) {
        if let Some(inner) = self {
            inner.reset()
        }
    }

    fn capacity(&self) -> usize {
        self.as_ref().map(|i| i.capacity()).unwrap_or(0)
    }

    fn really_dropped(&self) -> bool {
        self.as_ref().map(|i| i.really_dropped()).unwrap_or(true)
    }
}
