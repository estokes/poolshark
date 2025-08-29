use crossbeam_queue::ArrayQueue;
use fxhash::FxHashMap;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    any::{Any, TypeId},
    borrow::Borrow,
    cell::RefCell,
    cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd},
    collections::HashMap,
    default::Default,
    fmt::{self, Debug},
    hash::{Hash, Hasher},
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut},
    ptr,
    sync::{Arc, Weak},
};

use crate::{Poolable, RawPoolable};

pub mod arc;

thread_local! {
    static POOLS: RefCell<FxHashMap<TypeId, Box<dyn Any>>> =
        RefCell::new(HashMap::default());
}

/// Get a reference to a pool from the generic thread local pool set. This is
/// much more efficient than using `take_t` because the pool only needs to be
/// looked up once. `size` and `max` are only used if the pool doesn't already
/// exist. For more control over pools you can use `Pool` directly.
pub fn pool<T: Any + Poolable>(size: usize, max: usize) -> Pool<T> {
    POOLS.with_borrow_mut(|pools| {
        pools
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Pool::<T>::new(size, max)))
            .downcast_ref::<Pool<T>>()
            .unwrap()
            .clone()
    })
}

/// Take a poolable type T from the generic thread local pool set. It is much
/// more efficient to construct your own pools (or use `pool` and keep the pool
/// somewhere). size and max are the pool parameters used if the pool doesn't
/// already exist.
pub fn take_t<T: Any + Poolable>(size: usize, max: usize) -> Pooled<T> {
    POOLS.with_borrow_mut(|pools| {
        pools
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Pool::<T>::new(size, max)))
            .downcast_ref::<Pool<T>>()
            .unwrap()
            .take()
    })
}

/// A generic wrapper for pooled objects. This handles keeping track
/// of the pool pointer for you and allows you to wrap almost any
/// container type easily.
///
/// Most of the time, this is what you want to use.
#[derive(Clone)]
pub struct Pooled<T: Poolable> {
    pool: ManuallyDrop<WeakPool<Self>>,
    object: ManuallyDrop<T>,
}

impl<T: Poolable + Debug> fmt::Debug for Pooled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.object)
    }
}

unsafe impl<T: Poolable> RawPoolable for Pooled<T> {
    fn empty(pool: WeakPool<Self>) -> Self {
        Pooled {
            pool: ManuallyDrop::new(pool),
            object: ManuallyDrop::new(Poolable::empty()),
        }
    }

    fn reset(&mut self) {
        Poolable::reset(&mut *self.object)
    }

    fn capacity(&self) -> usize {
        Poolable::capacity(&*self.object)
    }

    fn really_drop(self) {
        drop(self.detach())
    }
}

impl<T: Poolable> Borrow<T> for Pooled<T> {
    fn borrow(&self) -> &T {
        &self.object
    }
}

impl Borrow<str> for Pooled<String> {
    fn borrow(&self) -> &str {
        &self.object
    }
}

impl<T: Poolable + PartialEq> PartialEq for Pooled<T> {
    fn eq(&self, other: &Pooled<T>) -> bool {
        self.object.eq(&other.object)
    }
}

impl<T: Poolable + Eq> Eq for Pooled<T> {}

impl<T: Poolable + PartialOrd> PartialOrd for Pooled<T> {
    fn partial_cmp(&self, other: &Pooled<T>) -> Option<Ordering> {
        self.object.partial_cmp(&other.object)
    }
}

impl<T: Poolable + Ord> Ord for Pooled<T> {
    fn cmp(&self, other: &Pooled<T>) -> Ordering {
        self.object.cmp(&other.object)
    }
}

impl<T: Poolable + Hash> Hash for Pooled<T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        Hash::hash(&self.object, state)
    }
}

impl<T: Poolable> Pooled<T> {
    /// Creates a `Pooled` that isn't connected to any pool. E.G. for
    /// branches where you know a given `Pooled` will always be empty.
    pub fn orphan(t: T) -> Self {
        Pooled {
            pool: ManuallyDrop::new(WeakPool::new()),
            object: ManuallyDrop::new(t),
        }
    }

    /// assign the `Pooled` to the specified pool. When it is dropped
    /// it will be placed in `pool` instead of the pool it was
    /// originally allocated from. If an orphan is assigned a pool it
    /// will no longer be orphaned.
    pub fn assign(&mut self, pool: &Pool<T>) {
        let old = mem::replace(&mut self.pool, ManuallyDrop::new(pool.downgrade()));
        drop(ManuallyDrop::into_inner(old))
    }

    /// detach the object from the pool, returning it.
    pub fn detach(self) -> T {
        let mut t = ManuallyDrop::new(self);
        unsafe {
            ManuallyDrop::drop(&mut t.pool);
            ManuallyDrop::take(&mut t.object)
        }
    }
}

impl<T: Poolable> AsRef<T> for Pooled<T> {
    fn as_ref(&self) -> &T {
        &self.object
    }
}

impl<T: Poolable> Deref for Pooled<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.object
    }
}

impl<T: Poolable> DerefMut for Pooled<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.object
    }
}

impl<T: Poolable> Drop for Pooled<T> {
    fn drop(&mut self) {
        if self.really_dropped() {
            match self.pool.upgrade() {
                Some(pool) => pool.insert(unsafe { ptr::read(self) }),
                None => unsafe {
                    ManuallyDrop::drop(&mut self.pool);
                    ManuallyDrop::drop(&mut self.object);
                },
            }
        }
    }
}

#[cfg(feature = "serde")]
impl<T: Poolable + Serialize> Serialize for Pooled<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.object.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: Poolable + DeserializeOwned + 'static> Deserialize<'de> for Pooled<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut t = take_t::<T>(1000, 1000);
        Self::deserialize_in_place(deserializer, &mut t)?;
        Ok(t)
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <T as Deserialize>::deserialize_in_place(deserializer, &mut place.object)
    }
}

#[derive(Debug)]
struct PoolInner<T: RawPoolable> {
    max_elt_capacity: usize,
    pool: ArrayQueue<T>,
}

impl<T: RawPoolable> Drop for PoolInner<T> {
    fn drop(&mut self) {
        while let Some(t) = self.pool.pop() {
            RawPoolable::really_drop(t)
        }
    }
}

pub struct WeakPool<T: RawPoolable>(Weak<PoolInner<T>>);

impl<T: RawPoolable> Debug for WeakPool<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<weak pool>")
    }
}

impl<T: RawPoolable> Clone for WeakPool<T> {
    fn clone(&self) -> Self {
        Self(Weak::clone(&self.0))
    }
}

impl<T: RawPoolable> WeakPool<T> {
    pub fn new() -> Self {
        WeakPool(Weak::new())
    }

    pub fn upgrade(&self) -> Option<RawPool<T>> {
        self.0.upgrade().map(RawPool)
    }
}

pub type Pool<T> = RawPool<Pooled<T>>;

/// a lock-free, thread-safe, dynamically-sized object pool.
///
/// this pool begins with an initial capacity and will continue
/// creating new objects on request when none are available. Pooled
/// objects are returned to the pool on destruction.
///
/// if, during an attempted return, a pool already has
/// `maximum_capacity` objects in the pool, the pool will throw away
/// that object.
#[derive(Debug)]
pub struct RawPool<T: RawPoolable>(Arc<PoolInner<T>>);

impl<T: RawPoolable> Clone for RawPool<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: RawPoolable> RawPool<T> {
    pub fn downgrade(&self) -> WeakPool<T> {
        WeakPool(Arc::downgrade(&self.0))
    }

    /// creates a new `Pool<T>`. this pool will retain up to
    /// `max_capacity` objects of size less than or equal to
    /// max_elt_capacity. Objects larger than max_elt_capacity will be
    /// deallocated immediatly.
    pub fn new(max_capacity: usize, max_elt_capacity: usize) -> RawPool<T> {
        RawPool(Arc::new(PoolInner {
            pool: ArrayQueue::new(max_capacity),
            max_elt_capacity,
        }))
    }

    /// try to take an element from the pool, return None if it is empty
    pub fn try_take(&self) -> Option<T> {
        self.0.pool.pop()
    }

    /// takes an item from the pool, creating one if none are available.
    pub fn take(&self) -> T {
        self.0
            .pool
            .pop()
            .unwrap_or_else(|| RawPoolable::empty(self.downgrade()))
    }

    /// Insert an object into the pool. The object may be dropped if
    /// the pool is at capacity, or the object has too much capacity.
    pub fn insert(&self, mut t: T) {
        let cap = t.capacity();
        if cap > 0 && cap <= self.0.max_elt_capacity {
            t.reset();
            if let Err(t) = self.0.pool.push(t) {
                RawPoolable::really_drop(t)
            }
        } else {
            RawPoolable::really_drop(t)
        }
    }

    /// Throw some pooled objects away. If the number of pooled objects is > 10%
    /// of the capacity then throw away 10% of the capacity. Otherwise throw
    /// away 1% of the capacity. Always throw away at least 1 object until the
    /// pool is empty.
    pub fn prune(&self) {
        let len = self.0.pool.len();
        let ten_percent = std::cmp::max(1, self.0.pool.capacity() / 10);
        let one_percent = std::cmp::max(1, ten_percent / 10);
        if len > ten_percent {
            for _ in 0..ten_percent {
                if let Some(v) = self.0.pool.pop() {
                    RawPoolable::really_drop(v)
                }
            }
        } else if len > one_percent {
            for _ in 0..one_percent {
                if let Some(v) = self.0.pool.pop() {
                    RawPoolable::really_drop(v)
                }
            }
        } else if len > 0 {
            if let Some(v) = self.0.pool.pop() {
                RawPoolable::really_drop(v)
            }
        }
    }
}
