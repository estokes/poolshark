//! Built-in [`Poolable`] and [`IsoPoolable`] implementations.
//!
//! This module provides pooling support for standard library types:
//!
//! - **Containers**: `Vec<T>`, `VecDeque<T>`, `HashMap<K, V>`, `HashSet<K>`
//! - **Strings**: `String`
//! - **Optional containers**: `Option<T>` where `T: Poolable`
//! - **IndexMap types** (with `indexmap` feature): `IndexMap<K, V>`, `IndexSet<K>`
//!
//! You don't need to import anything from this module - the implementations are
//! automatically available when you use the pooled types.
use super::{Discriminant, IsoPoolable, Poolable, location_id};
#[cfg(feature = "indexmap")]
use indexmap::{IndexMap, IndexSet};
use std::{
    alloc::Layout,
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

        unsafe impl<K, V, R> IsoPoolable for $ty<K, V, R>
        where
            K: Hash + Eq,
            R: Default + BuildHasher,
        {
            const DISCRIMINANT: Option<Discriminant> = {
                assert!(Layout::new::<R>().size() == 0);
                Discriminant::new_p2::<K, V>(location_id!())
            };
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

        unsafe impl<K, R> IsoPoolable for $ty<K, R>
        where
            K: Hash + Eq,
            R: Default + BuildHasher,
        {
            const DISCRIMINANT: Option<Discriminant> = Discriminant::new_p2::<K, R>(location_id!());
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

unsafe impl<T> IsoPoolable for Vec<T> {
    const DISCRIMINANT: Option<Discriminant> = Discriminant::new_p1::<T>(location_id!());
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

unsafe impl<T> IsoPoolable for VecDeque<T> {
    const DISCRIMINANT: Option<Discriminant> = Discriminant::new_p1::<T>(location_id!());
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

unsafe impl IsoPoolable for String {
    const DISCRIMINANT: Option<Discriminant> = Discriminant::new(location_id!());
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

    fn really_dropped(&mut self) -> bool {
        self.as_mut().map(|i| i.really_dropped()).unwrap_or(true)
    }
}
