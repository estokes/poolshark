//! Thread safe, lock free, global object pools
use crate::{Discriminant, IsoPoolable, Opaque, Poolable, RawPoolable};
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
    sync::{Arc, LazyLock, Mutex, Weak},
};

pub mod arc;

thread_local! {
    static POOLS: RefCell<FxHashMap<Discriminant, Opaque>> =
        RefCell::new(HashMap::default());
}

const DEFAULT_SIZES: (usize, usize) = (1024, 1024);

static SIZES: LazyLock<Mutex<FxHashMap<Discriminant, (usize, usize)>>> =
    LazyLock::new(|| Mutex::new(FxHashMap::default()));

// This is safe because:
// 1. Containers are reset before being returned to pools, so they contain no values
// 2. We only reuse pools for types with identical memory layouts (same size/alignment via Discriminant)
// 3. The Opaque wrapper ensures proper cleanup when the thread local is destroyed
fn with_pool<T, R, F>(sizes: Option<(usize, usize)>, f: F) -> R
where
    T: IsoPoolable,
    F: FnOnce(Option<&Pool<T>>) -> R,
{
    let mut f = Some(f);
    // if the user implements Drop on the pooled item and tries to put it back
    // in the pool then we will end up calling ourselves recursively from the
    // pool destructor. This is why we must use try_with on the thread local
    let res = POOLS.try_with(|pools| match pools.try_borrow_mut() {
        Err(_) => (f.take().unwrap())(None),
        Ok(mut pools) => match T::DISCRIMINANT {
            Some(d) => {
                let pool = pools.entry(d).or_insert_with(|| {
                    let (size, cap) = sizes.unwrap_or_else(|| {
                        SIZES
                            .lock()
                            .unwrap()
                            .get(&d)
                            .map(|(s, c)| (*s, *c))
                            .unwrap_or(DEFAULT_SIZES)
                    });
                    let b = Box::new(Pool::<T>::new(size, cap));
                    let t = Box::into_raw(b) as *mut ();
                    let drop = Some(Box::new(|t: *mut ()| unsafe {
                        drop(Box::from_raw(t as *mut Pool<T>))
                    }) as Box<dyn FnOnce(*mut ())>);
                    Opaque { t, drop }
                });
                (f.take().unwrap())(unsafe { Some(&*(pool.t as *mut Pool<T>)) })
            }
            None => (f.take().unwrap())(None),
        },
    });
    match res {
        Err(_) => (f.take().unwrap())(None),
        Ok(r) => r,
    }
}

/// Clear all thread local pools on this thread. Note this will happen
/// automatically when the thread dies.
pub fn clear() {
    POOLS.with_borrow_mut(|pools| pools.clear())
}

/// Delete the thread local pool for the specified `T`. Note, this will
/// happen automatically when the current thread dies.
pub fn clear_type<T: IsoPoolable>() {
    POOLS.with_borrow_mut(|pools| {
        if let Some(d) = T::DISCRIMINANT {
            pools.remove(&d);
        }
    })
}

/// Set the pool size for the global pools of `T`. Pools that have already been
/// created will not be resized, but new pools (on new threads) will use the
/// specified size as their max size. If you wish to resize an existing pool you
/// can first clear_type (or clear) and then set_size.
pub fn set_size<T: IsoPoolable>(max_pool_size: usize, max_element_capacity: usize) {
    if let Some(d) = T::DISCRIMINANT {
        SIZES
            .lock()
            .unwrap()
            .insert(d, (max_pool_size, max_element_capacity));
    }
}

/// get the max pool size and the max element capacity for a given type. If
/// get_size returns None then the type will not be pooled.
pub fn get_size<T: IsoPoolable>() -> Option<(usize, usize)> {
    T::DISCRIMINANT.map(|d| {
        SIZES
            .lock()
            .unwrap()
            .get(&d)
            .map(|(s, c)| (*s, *c))
            .unwrap_or(DEFAULT_SIZES)
    })
}

fn take_inner<T: IsoPoolable>(sizes: Option<(usize, usize)>) -> GPooled<T> {
    with_pool(sizes, |pool| {
        pool.map(|p| p.take())
            .unwrap_or_else(|| GPooled::orphan(T::empty()))
    })
}

/// Take a `T` from the thread local global pool, if there is no pool for `T`j
/// or there are no `T`s pooled then create a new empty `T`. If `T` has no
/// discrimiant return an orphan.
pub fn take<T: IsoPoolable>() -> GPooled<T> {
    take_inner(None)
}

/// Take a `T` from the thread local global pool, if there is no pool for `T`j
/// or there are no `T`s pooled then create a new empty `T`. If `T` has no
/// discrimiant return an orphan. Also set the pool sizes for this type if they have
/// not already been set.
pub fn take_sz<T: IsoPoolable>(max: usize, max_elements: usize) -> GPooled<T> {
    take_inner(Some((max, max_elements)))
}

/// Get a reference to the thread local global pool of `T`s if `T` has a
/// discriminant. You can use [get_size], [set_size], [clear] and [clear_type]
/// to control these global pools on the current thread. This function unlike
/// [pool_any] does not require `T` to implement [Any], so you could use it to
/// pool a type like `HashMap<&str, &str>`.
pub fn pool<T: IsoPoolable>() -> Option<Pool<T>> {
    with_pool(None, |pool| pool.cloned())
}

/// Get a reference to the thread local global pool of `T`s if `T` has a
/// discriminant. You can use [get_size], [set_size], [clear] and [clear_type]
/// to control these global pools on the current thread. This function unlike
/// [pool_any] does not require `T` to implement [Any], so you could use it to
/// pool a type like `HashMap<&str, &str>`. Also set the pool sizes for this
/// type if they have not already been set
pub fn pool_sz<T: IsoPoolable>(max: usize, max_elements: usize) -> Option<Pool<T>> {
    with_pool(Some((max, max_elements)), |pool| pool.cloned())
}

thread_local! {
    static ANY_POOLS: RefCell<FxHashMap<TypeId, Box<dyn Any>>> =
        RefCell::new(HashMap::default());
}

/// Get a reference to a pool from the generic thread local pool set for any
/// type that implements [Any] + [Poolable]. Note this is a different set of
/// pools vs ones returned by [pool]. If your container type implements both
/// [IsoPoolable] and [Any] then you can choose either of these two pool sets, it
/// doesn't really matter for performance which one you choose as long as your
/// choice is consistent.
pub fn pool_any<T: Any + Poolable>(size: usize, max: usize) -> Pool<T> {
    ANY_POOLS.with_borrow_mut(|pools| {
        pools
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Pool::<T>::new(size, max)))
            .downcast_ref::<Pool<T>>()
            .unwrap()
            .clone()
    })
}

/// Take a poolable type `T` that also implements [Any] from the generic thread
/// local global pool set. It is much more efficient to use [take] if your container
/// type implements [IsoPoolable], and even more efficent to use [pool] or
/// [pool_any] and store the pool somewhere.
pub fn take_any<T: Any + Poolable>(size: usize, max: usize) -> GPooled<T> {
    ANY_POOLS.with_borrow_mut(|pools| {
        pools
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Pool::<T>::new(size, max)))
            .downcast_ref::<Pool<T>>()
            .unwrap()
            .take()
    })
}

/// A wrapper for globally pooled objects.
///
/// Globally pooled objects differ from locally pooled objects because the
/// glocal pools can be shared between threads. For example in the case of a
/// producer thread producing objects and sending them to consumer threads an
/// [LPooled](crate::local::LPooled) object would have no value. The dropped objects
/// would just accumulate in the consumers and would never be available to the
/// producer. In such a case a GPooled object would be useful because when
/// dropped it will always return to the pool it was created from, in our case
/// the producer, making it available for reuse.
///
/// Conversely if your objects will mostly be produced and consumed on the same
/// set of threads, and your containers can implement [IsoPoolable] then consider
/// using [LPooled](crate::local::LPooled) objects, as they are considerably faster.
///
/// `GPooled` has an overhead of 1 machine work on the stack to store the pool
/// pointer.
#[derive(Clone)]
pub struct GPooled<T: Poolable> {
    pool: ManuallyDrop<WeakPool<Self>>,
    object: ManuallyDrop<T>,
}

impl<T: Poolable + Debug> fmt::Debug for GPooled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.object)
    }
}

impl<T: IsoPoolable> Default for GPooled<T> {
    fn default() -> Self {
        take()
    }
}

unsafe impl<T: Poolable> RawPoolable for GPooled<T> {
    fn empty(pool: WeakPool<Self>) -> Self {
        Self {
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

impl<T: Poolable> Borrow<T> for GPooled<T> {
    fn borrow(&self) -> &T {
        &self.object
    }
}

impl Borrow<str> for GPooled<String> {
    fn borrow(&self) -> &str {
        &self.object
    }
}

impl<T: Poolable + PartialEq> PartialEq for GPooled<T> {
    fn eq(&self, other: &GPooled<T>) -> bool {
        self.object.eq(&other.object)
    }
}

impl<T: Poolable + Eq> Eq for GPooled<T> {}

impl<T: Poolable + PartialOrd> PartialOrd for GPooled<T> {
    fn partial_cmp(&self, other: &GPooled<T>) -> Option<Ordering> {
        self.object.partial_cmp(&other.object)
    }
}

impl<T: Poolable + Ord> Ord for GPooled<T> {
    fn cmp(&self, other: &GPooled<T>) -> Ordering {
        self.object.cmp(&other.object)
    }
}

impl<T: Poolable + Hash> Hash for GPooled<T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        Hash::hash(&self.object, state)
    }
}

impl<T: Poolable> GPooled<T> {
    /// Creates a `GPooled` that isn't connected to any pool. E.G. for
    /// branches where you know a given `Pooled` will always be empty.
    pub fn orphan(t: T) -> Self {
        Self {
            pool: ManuallyDrop::new(WeakPool::new()),
            object: ManuallyDrop::new(t),
        }
    }

    /// assign the `GPooled` to the specified pool. When it is dropped
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

impl<T: Poolable> AsRef<T> for GPooled<T> {
    fn as_ref(&self) -> &T {
        &self.object
    }
}

impl<T: Poolable> Deref for GPooled<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.object
    }
}

impl<T: Poolable> DerefMut for GPooled<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.object
    }
}

impl<T: Poolable> Drop for GPooled<T> {
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
impl<T: Poolable + Serialize> Serialize for GPooled<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.object.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: Poolable + DeserializeOwned + 'static> Deserialize<'de> for GPooled<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut t = take_any::<T>(1024, 1024);
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

pub type Pool<T> = RawPool<GPooled<T>>;

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

    /// creates a new `RawPool<T>`. this pool will retain up to
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
