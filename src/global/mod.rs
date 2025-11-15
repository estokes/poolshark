//! Lock-free global object pools for cross-thread pooling.
//!
//! Global pools ensure objects always return to their origin pool, regardless of which
//! thread drops them. This is essential for producer-consumer patterns where one thread
//! creates objects and other threads consume them.
//!
//! # When to Use
//!
//! Use global pools when:
//! - One thread primarily allocates objects while others consume them
//! - You need objects to return to a specific pool regardless of which thread drops them
//! - You have a producer-consumer pattern across threads
//!
//! Otherwise, prefer [`crate::local`] pools for better performance.
//!
//! # Examples
//!
//! ## Using a static global pool
//!
//! ```
//! use poolshark::global::{Pool, GPooled};
//! use std::sync::LazyLock;
//!
//! static STRINGS: LazyLock<Pool<String>> = LazyLock::new(|| Pool::new(1024, 4096));
//!
//! fn create_message() -> GPooled<String> {
//!     let mut s = STRINGS.take();
//!     s.push_str("Hello, world!");
//!     s
//! }
//! ```
//!
//! ## Using thread-local global pools
//!
//! ```
//! use poolshark::global;
//! use std::collections::HashMap;
//!
//! // Take from thread-local global pool
//! let map = global::take::<HashMap<String, i32>>();
//! ```
use crate::{Discriminant, IsoPoolable, Opaque, Poolable, RawPoolable};
use crossbeam_queue::ArrayQueue;
use fxhash::FxHashMap;
#[cfg(feature = "serde")]
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    any::{Any, TypeId},
    borrow::Borrow,
    cell::RefCell,
    cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd},
    collections::HashMap,
    default::Default,
    fmt::{self, Debug, Display},
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

/// Clear all thread local global pools on this thread.
///
/// Note this will happen automatically when the thread dies.
pub fn clear() {
    POOLS.with_borrow_mut(|pools| pools.clear())
}

/// Delete the thread local pool for the specified `T`.
///
/// Note this will happen automatically when the current thread dies.
pub fn clear_type<T: IsoPoolable>() {
    POOLS.with_borrow_mut(|pools| {
        if let Some(d) = T::DISCRIMINANT {
            pools.remove(&d);
        }
    })
}

/// Set the pool size for the global pools of `T`.
///
/// Pools that have already been created will not be resized, but new pools (on new threads)
/// will use the specified size as their max size. If you wish to resize an existing pool you
/// can first clear_type (or clear) and then set_size.
pub fn set_size<T: IsoPoolable>(max_pool_size: usize, max_element_capacity: usize) {
    if let Some(d) = T::DISCRIMINANT {
        SIZES.lock().unwrap().insert(d, (max_pool_size, max_element_capacity));
    }
}

/// Get the max pool size and max element capacity for a given type.
///
/// If get_size returns None then the type will not be pooled.
pub fn get_size<T: IsoPoolable>() -> Option<(usize, usize)> {
    T::DISCRIMINANT.map(|d| {
        SIZES.lock().unwrap().get(&d).map(|(s, c)| (*s, *c)).unwrap_or(DEFAULT_SIZES)
    })
}

fn take_inner<T: IsoPoolable>(sizes: Option<(usize, usize)>) -> GPooled<T> {
    with_pool(sizes, |pool| {
        pool.map(|p| p.take()).unwrap_or_else(|| GPooled::orphan(T::empty()))
    })
}

/// Take a `T` from the thread local global pool.
///
/// If there is no pool for `T` or there are no `T`s pooled then create a new empty `T`.
/// If `T` has no discriminant return an orphan.
pub fn take<T: IsoPoolable>() -> GPooled<T> {
    take_inner(None)
}

/// Take a `T` from the thread local global pool with custom pool sizes.
///
/// If there is no pool for `T` or there are no `T`s pooled then create a new empty `T`.
/// If `T` has no discriminant return an orphan. Also set the pool sizes for this type
/// if they have not already been set.
pub fn take_sz<T: IsoPoolable>(max: usize, max_elements: usize) -> GPooled<T> {
    take_inner(Some((max, max_elements)))
}

/// Get a reference to the thread local global pool of `T`s.
///
/// Returns `None` if `T` has no discriminant. You can use [get_size], [set_size],
/// [clear] and [clear_type] to control these global pools on the current thread.
/// This function unlike [pool_any] does not require `T` to implement [Any], so you
/// could use it to pool a type like `HashMap<&str, &str>`.
pub fn pool<T: IsoPoolable>() -> Option<Pool<T>> {
    with_pool(None, |pool| pool.cloned())
}

/// Get a reference to the thread local global pool of `T`s with custom sizes.
///
/// Returns `None` if `T` has no discriminant. You can use [get_size], [set_size],
/// [clear] and [clear_type] to control these global pools on the current thread.
/// This function unlike [pool_any] does not require `T` to implement [Any], so you
/// could use it to pool a type like `HashMap<&str, &str>`. Also sets the pool sizes
/// for this type if they have not already been set.
pub fn pool_sz<T: IsoPoolable>(max: usize, max_elements: usize) -> Option<Pool<T>> {
    with_pool(Some((max, max_elements)), |pool| pool.cloned())
}

thread_local! {
    static ANY_POOLS: RefCell<FxHashMap<TypeId, Box<dyn Any>>> =
        RefCell::new(HashMap::default());
}

/// Get a reference to a pool from the generic thread local pool set.
///
/// This works for any type that implements [Any] + [Poolable]. Note this is a different
/// set of pools vs ones returned by [pool]. If your container type implements both
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

/// Take a poolable type `T` from the generic thread local pool set.
///
/// This works for types that implement [Any] + [Poolable]. It is much more efficient
/// to use [take] if your container type implements [IsoPoolable], and even more efficient
/// to use [pool] or [pool_any] and store the pool somewhere.
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

/// A wrapper for globally pooled objects with cross-thread pool affinity.
///
/// `GPooled<T>` ensures objects always return to their origin pool, regardless of which
/// thread drops them. This is essential for producer-consumer patterns where one thread
/// creates objects and other threads consume them.
///
/// # When to Use
///
/// Use `GPooled` when:
/// - One thread primarily creates objects, other threads consume them
/// - You need objects to return to a specific pool
/// - You have a producer-consumer pattern across threads
///
/// Otherwise, prefer [`LPooled`](crate::local::LPooled) for better performance.
///
/// # Example
///
/// ```
/// use poolshark::global::{Pool, GPooled};
/// use std::sync::LazyLock;
///
/// // Shared pool for cross-thread usage
/// static MESSAGES: LazyLock<Pool<String>> = LazyLock::new(|| Pool::new(1024, 4096));
///
/// fn producer() -> GPooled<String> {
///     let mut msg = MESSAGES.take();
///     msg.push_str("Hello from producer");
///     msg  // Can be sent to consumer thread
/// }
///
/// fn consumer(msg: GPooled<String>) {
///     println!("{}", msg);
///     // Dropped here, returns to MESSAGES pool (not consumer's thread-local pool)
/// }
/// ```
///
/// # Behavior
///
/// - **Pool affinity**: Always returns to the pool it was created from
/// - **Thread-safe**: Can be sent between threads
/// - **Overhead**: One word (8 bytes on 64-bit) to store pool pointer
/// - **Lock-free**: Uses `crossbeam` lock-free queues
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

impl<T: Poolable + Display> fmt::Display for GPooled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &*self.object)
    }
}

impl<T: IsoPoolable> Default for GPooled<T> {
    fn default() -> Self {
        take()
    }
}

impl<T: IsoPoolable> GPooled<T> {
    pub fn take() -> Self {
        take()
    }

    pub fn take_sz(max: usize, max_elements: usize) -> Self {
        take_sz(max, max_elements)
    }
}

impl<T: IsoPoolable + Extend<E>, E> Extend<E> for GPooled<T> {
    fn extend<I: IntoIterator<Item = E>>(&mut self, iter: I) {
        self.object.extend(iter)
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
    /// Creates a `GPooled` that isn't connected to any pool.
    ///
    /// Useful for branches where you know a given `Pooled` will always be empty.
    pub fn orphan(t: T) -> Self {
        Self { pool: ManuallyDrop::new(WeakPool::new()), object: ManuallyDrop::new(t) }
    }

    /// Assign the `GPooled` to the specified pool.
    ///
    /// When dropped, it will be placed in `pool` instead of the pool it was originally
    /// allocated from. If an orphan is assigned a pool it will no longer be orphaned.
    pub fn assign(&mut self, pool: &Pool<T>) {
        let old = mem::replace(&mut self.pool, ManuallyDrop::new(pool.downgrade()));
        drop(ManuallyDrop::into_inner(old))
    }

    /// Detach the object from the pool, returning the inner value.
    ///
    /// The detached object will not be returned to any pool when dropped.
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

/// A weak reference to a global Pool
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

/// A global pool
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

    /// Creates a new `RawPool<T>`.
    ///
    /// This pool will retain up to `max_capacity` objects of size less than or equal to
    /// `max_elt_capacity`. Objects larger than `max_elt_capacity` will be deallocated immediately.
    pub fn new(max_capacity: usize, max_elt_capacity: usize) -> RawPool<T> {
        RawPool(Arc::new(PoolInner {
            pool: ArrayQueue::new(max_capacity),
            max_elt_capacity,
        }))
    }

    /// Try to take an element from the pool.
    ///
    /// Returns `None` if the pool is empty.
    pub fn try_take(&self) -> Option<T> {
        self.0.pool.pop()
    }

    /// Takes an item from the pool.
    ///
    /// Creates a new item if none are available.
    pub fn take(&self) -> T {
        self.0.pool.pop().unwrap_or_else(|| RawPoolable::empty(self.downgrade()))
    }

    /// Insert an object into the pool.
    ///
    /// The object may be dropped if the pool is at capacity or if the object
    /// has too much capacity.
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

    /// Throw away some pooled objects to reduce memory usage.
    ///
    /// If the number of pooled objects is > 10% of the capacity then throw away 10%
    /// of the capacity. Otherwise throw away 1% of the capacity. Always throw away
    /// at least 1 object until the pool is empty.
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
