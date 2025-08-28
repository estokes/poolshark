use super::{ContainerId, Discriminant, LocalPoolable, Poolable};
#[cfg(feature = "indexmap")]
use indexmap::{IndexMap, IndexSet};
use std::{
    cmp::Eq,
    collections::{HashMap, HashSet, VecDeque},
    default::Default,
    hash::{BuildHasher, Hash},
};

macro_rules! impl_hashmap {
    ($ty:ident, $id:expr) => {
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

        unsafe impl<K, V, R> LocalPoolable for $ty<K, V, R>
        where
            K: Hash + Eq,
            R: Default + BuildHasher,
        {
            fn discriminant() -> Option<Discriminant> {
                Discriminant::new_p3::<K, V, R>($id)
            }
        }
    };
}

impl_hashmap!(HashMap, ContainerId(0));
#[cfg(feature = "indexmap")]
impl_hashmap!(IndexMap, ContainerId(1));

macro_rules! impl_hashset {
    ($ty:ident, $id:expr) => {
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

        unsafe impl<K, R> LocalPoolable for $ty<K, R>
        where
            K: Hash + Eq,
            R: Default + BuildHasher,
        {
            fn discriminant() -> Option<Discriminant> {
                Discriminant::new_p2::<K, R>($id)
            }
        }
    };
}

impl_hashset!(HashSet, ContainerId(2));
#[cfg(feature = "indexmap")]
impl_hashset!(IndexSet, ContainerId(3));

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

unsafe impl<T> LocalPoolable for Vec<T> {
    fn discriminant() -> Option<Discriminant> {
        Discriminant::new_p1::<T>(ContainerId(4))
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

unsafe impl<T> LocalPoolable for VecDeque<T> {
    fn discriminant() -> Option<Discriminant> {
        Discriminant::new_p1::<T>(ContainerId(5))
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

unsafe impl LocalPoolable for String {
    fn discriminant() -> Option<Discriminant> {
        Discriminant::new(ContainerId(6))
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

    fn really_dropped(&mut self) -> bool {
        self.as_mut().map(|i| i.really_dropped()).unwrap_or(true)
    }
}

unsafe impl<T: LocalPoolable> LocalPoolable for Option<T> {
    fn discriminant() -> Option<Discriminant> {
        let inner = T::discriminant()?;
        Some(Discriminant {
            container: ContainerId(7),
            elements: inner.elements,
        })
    }
}
