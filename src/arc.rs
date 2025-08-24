use super::{Poolable, RawPool, RawPoolable, WeakPool};
use core::fmt;
use std::{cmp::Eq, fmt::Debug, hash::Hash, mem::ManuallyDrop, ops::Deref, ptr};

macro_rules! impl_arc {
    ($name:ident, $inner:ident, $uniq:expr) => {
        #[derive(Clone)]
        pub struct $name<T: Poolable> {
            inner: ManuallyDrop<$inner<(WeakPool<Self>, T)>>,
        }

        unsafe impl<T: Poolable> RawPoolable for $name<T> {
            fn empty(pool: super::WeakPool<Self>) -> Self {
                Self {
                    inner: ManuallyDrop::new($inner::new((pool, T::empty()))),
                }
            }

            fn capacity(&self) -> usize {
                1
            }

            fn reset(&mut self) {
                $inner::get_mut(&mut self.inner).unwrap().1.reset()
            }

            fn really_drop(self) {
                let mut t = ManuallyDrop::new(self);
                unsafe { ManuallyDrop::drop(&mut t.inner) }
            }
        }

        impl<T: Poolable> Drop for $name<T> {
            fn drop(&mut self) {
                if !$uniq(&mut self.inner) {
                    unsafe { ManuallyDrop::drop(&mut self.inner) }
                } else {
                    match self.inner.0.upgrade() {
                        None => unsafe { ManuallyDrop::drop(&mut self.inner) },
                        Some(pool) => pool.insert(unsafe { ptr::read(self) }),
                    }
                }
            }
        }

        impl<T: Poolable> Deref for $name<T> {
            type Target = T;

            fn deref(&self) -> &Self::Target {
                &self.inner.1
            }
        }

        impl<T: Poolable + Debug> fmt::Debug for $name<T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.inner.1.fmt(f)
            }
        }

        impl<T: Poolable + PartialEq> PartialEq for $name<T> {
            fn eq(&self, other: &Self) -> bool {
                self.inner.1 == other.inner.1
            }
        }

        impl<T: Poolable + Eq> Eq for $name<T> {}

        impl<T: Poolable + PartialOrd> PartialOrd for $name<T> {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                self.inner.1.partial_cmp(&other.inner.1)
            }
        }

        impl<T: Poolable + Ord> Ord for $name<T> {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.inner.1.cmp(&other.inner.1)
            }
        }

        impl<T: Poolable + Hash> Hash for $name<T> {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.inner.1.hash(state)
            }
        }

        impl<T: Poolable> $name<T> {
            /// allocate a new arc from the specified pool and return it containing v
            pub fn new(pool: &RawPool<Self>, v: T) -> Self {
                let mut t = pool.take();
                // values in the pool are guaranteed to be unique
                *Self::get_mut(&mut t).unwrap() = v;
                t
            }

            /// if the Arc is unique, get a mutable pointer to the inner T,
            /// otherwise return None
            pub fn get_mut(&mut self) -> Option<&mut T> {
                match $inner::get_mut(&mut *self.inner) {
                    Some((_, t)) => Some(t),
                    None => None,
                }
            }

            pub fn strong_count(&self) -> usize {
                $inner::strong_count(&*self.inner)
            }

            pub fn as_ptr(&self) -> *const (WeakPool<Self>, T) {
                $inner::as_ptr(&*self.inner)
            }
        }

        impl<T: Poolable + Clone> $name<T> {
            pub fn make_mut(&mut self) -> &mut T {
                if $inner::get_mut(&mut self.inner).is_some() {
                    return &mut $inner::get_mut(&mut self.inner).unwrap().1;
                }
                match self.inner.0.upgrade() {
                    None => &mut $inner::make_mut(&mut self.inner).1,
                    Some(p) => {
                        let v = self.inner.1.clone();
                        *self = Self::new(&p, v);
                        &mut $inner::get_mut(&mut self.inner).unwrap().1
                    }
                }
            }
        }
    };
}

#[cfg(feature = "triomphe")]
use triomphe::Arc as TArcInner;

#[cfg(feature = "triomphe")]
impl_arc!(TArc, TArcInner, TArcInner::is_unique);

#[cfg(feature = "triomphe")]
impl<T: Poolable + Send + Sync> TArc<T> {
    pub fn is_unique(&self) -> bool {
        self.inner.is_unique()
    }
}

use std::sync::Arc as ArcInner;
impl_arc!(Arc, ArcInner, |a| ArcInner::get_mut(a).is_some());

impl<T: Poolable + Clone> Arc<T> {}
