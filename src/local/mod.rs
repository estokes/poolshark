//! Fast thread-local object pools.
//!
//! Local pools are significantly faster than global pools because they avoid atomic
//! operations. Objects are returned to the pool of whichever thread drops them, making
//! this ideal for workloads where objects stay on the same thread or are evenly
//! distributed across threads.
//!
//! # When to Use
//!
//! Use local pools (the default choice) when:
//! - Objects are created and dropped on the same thread
//! - All threads allocate and free objects roughly equally
//! - You want maximum performance
//!
//! If you have a producer-consumer pattern where one thread creates and other threads
//! consume, use [`crate::global`] pools instead.
//!
//! # Examples
//!
//! ## Basic usage
//!
//! ```
//! use poolshark::local::LPooled;
//! use std::collections::HashMap;
//!
//! let mut map: LPooled<HashMap<String, i32>> = LPooled::take();
//! map.insert("key".to_string(), 42);
//! // When dropped, map is cleared and returned to the thread-local pool
//! ```
//!
//! ## Reusing allocations across function calls
//!
//! ```no_run
//! use poolshark::local::LPooled;
//! use std::collections::HashSet;
//!
//! fn process_batch(items: &[String]) -> LPooled<Vec<String>> {
//!     // This HashSet will be reused across calls on the same thread
//!     let mut seen: LPooled<HashSet<String>> = LPooled::take();
//!     // vecs will be reused when the caller drops them
//!     items.iter().filter(|s| seen.insert(s.to_string())).cloned().collect()
//! }
//! ```
//!
//! # How It Works
//!
//! - **Thread safety**: `LPooled<T>` is `Send + Sync` whenever `T` is `Send + Sync`, making it
//!   safe to pass pooled objects between threads
//! - When dropped, objects return to the pool of the dropping thread (not necessarily
//!   the creating thread)
//! - Pools are thread-local, so each thread maintains its own pool per layout
//! - Types must implement [`crate::IsoPoolable`] (all standard containers already do)
//! - If `T` and `U` have the same size and alignment then `LPooled<Vec<T>>` can be reused
//!   as `LPooled<Vec<U>>`
//! - References are allowed! `LPooled<Vec<&T>>` will work, and will
//!   reuse any `Vec<&X>` where `&X` has the same size and alignment as `&T` (in
//!   current rust that means there will be a pool for thin references and a
//!   pool for fat references).

use crate::{Discriminant, IsoPoolable, Opaque};
use fxhash::FxHashMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    borrow::Borrow,
    cell::RefCell,
    collections::HashMap,
    fmt::Display,
    hash::Hash,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    ptr,
    sync::{LazyLock, Mutex},
};

struct Pool<T: IsoPoolable> {
    max: usize,
    max_capacity: usize,
    data: Vec<T>,
}

impl<T: IsoPoolable> Pool<T> {
    fn new(max: usize, max_capacity: usize) -> Self {
        Self { max, max_capacity, data: Vec::with_capacity(max) }
    }
}

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
    F: FnOnce(Option<&mut Pool<T>>) -> R,
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
                (f.take().unwrap())(unsafe { Some(&mut *(pool.t as *mut Pool<T>)) })
            }
            None => (f.take().unwrap())(None),
        },
    });
    match res {
        Err(_) => (f.take().unwrap())(None),
        Ok(r) => r,
    }
}

/// Clear all thread local pools on this thread.
///
/// Note this will happen automatically when the thread dies.
pub fn clear() {
    POOLS.with_borrow_mut(|pools| pools.clear())
}

/// Delete the thread local pool for the specified type.
///
/// This will happen automatically when the current thread dies.
pub fn clear_type<T: IsoPoolable>() {
    POOLS.with_borrow_mut(|pools| {
        if let Some(d) = T::DISCRIMINANT {
            pools.remove(&d);
        }
    })
}

/// Set the pool size for this type.
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

fn take_inner<T: IsoPoolable>(sizes: Option<(usize, usize)>) -> T {
    with_pool(sizes, |pool| pool.and_then(|p| p.data.pop())).unwrap_or_else(|| T::empty())
}

/// Take a T from the pool.
///
/// If there is no pool for T or there are no Ts pooled then create a new empty T.
pub fn take<T: IsoPoolable>() -> T {
    take_inner(None)
}

/// Take a T from the pool with custom pool sizes.
///
/// If there is no pool for T or there are no Ts pooled then create a new empty T.
/// Configures the max size and max_elt size of the pool if it has not already been created.
pub fn take_sz<T: IsoPoolable>(max: usize, max_elt: usize) -> T {
    take_inner(Some((max, max_elt)))
}

unsafe fn insert_raw_inner<T: IsoPoolable>(
    sizes: Option<(usize, usize)>,
    t: T,
) -> Option<T> {
    with_pool(sizes, |pool| match pool {
        Some(pool) if pool.data.len() < pool.max && t.capacity() <= pool.max_capacity => {
            pool.data.push(t);
            None
        }
        None | Some(_) => Some(t),
    })
}

/// Insert a T into the pool without resetting it.
///
/// If there is no space in the pool available to hold T then return it, otherwise return None.
/// Does not reset T, the caller is responsible for resetting T. If you do not, horrible things can happen.
///
/// # Safety
///
/// The caller must ensure that T is properly reset before calling this function.
pub unsafe fn insert_raw<T: IsoPoolable>(t: T) -> Option<T> {
    unsafe { insert_raw_inner(None, t) }
}

/// Insert a T into the pool without resetting it, with custom pool sizes.
///
/// If there is no space in the pool available to hold T then return it, otherwise return None.
/// Does not reset T, the caller is responsible for resetting T. If you do not, horrible things can happen.
/// Also sets the max pool size and max_elt size if the pool has not been initialized yet.
///
/// # Safety
///
/// The caller must ensure that T is properly reset before calling this function.
pub unsafe fn insert_raw_sz<T: IsoPoolable>(
    max: usize,
    max_elt: usize,
    t: T,
) -> Option<T> {
    unsafe { insert_raw_inner(Some((max, max_elt)), t) }
}

/// Insert a T into the pool.
///
/// If there is no space in the pool available to hold T then return it, otherwise return None.
/// T will be reset before it is inserted into the pool. Reset must ensure that T is EMPTY.
pub fn insert<T: IsoPoolable>(mut t: T) -> Option<T> {
    t.reset();
    unsafe { insert_raw(t) }
}

/// Insert a T into the pool with custom pool sizes.
///
/// If there is no space in the pool available to hold T then return it, otherwise return None.
/// T will be reset before it is inserted into the pool. Reset must ensure that T is EMPTY.
pub fn insert_sz<T: IsoPoolable>(max: usize, max_elt: usize, mut t: T) -> Option<T> {
    t.reset();
    unsafe { insert_raw_inner(Some((max, max_elt)), t) }
}

/// A zero-cost wrapper for thread-local pooled objects.
///
/// `LPooled<T>` automatically returns objects to the thread-local pool when dropped.
/// This is the recommended default for most use cases as it's faster than [`GPooled`](crate::global::GPooled).
///
/// # When to Use
///
/// - Default choice for pooling
/// - Objects created and dropped on the same thread
/// - Maximum performance is important
///
/// For producer-consumer patterns where one thread creates and other threads consume,
/// use [`GPooled`](crate::global::GPooled) instead.
///
/// # Example
///
/// ``` no_run
/// use poolshark::local::LPooled;
/// use std::collections::HashMap;
///
/// fn process_request(data: &[(&str, i32)]) -> LPooled<HashMap<String, i32>> {
///     // will reuse dropped HashMaps
///     let mut map: LPooled<HashMap<String, i32>> = LPooled::take();
///     for (k, v) in data {
///         map.insert(k.to_string(), *v);
///     }
///     map
/// }
/// ```
///
/// # Behavior
///
/// - **Minimal overhead**: Same size as `T` on the stack, with thread-local lookup cost on drop and take
/// - **Thread-safe**: Can be sent between threads (implements `Send + Sync` if `T` does)
/// - **Drop behavior**: Returns to the pool of whichever thread drops it
/// - **Automatic**: No manual pool management required
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LPooled<T: IsoPoolable>(ManuallyDrop<T>);

impl<T: IsoPoolable + Display> Display for LPooled<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &*self.0)
    }
}

impl<T: IsoPoolable> Borrow<T> for LPooled<T> {
    fn borrow(&self) -> &T {
        &self.0
    }
}

impl Borrow<str> for LPooled<String> {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<T: IsoPoolable> Default for LPooled<T> {
    fn default() -> Self {
        Self::take()
    }
}

impl<T: IsoPoolable> LPooled<T> {
    /// Take an object from the pool, or create one if the pool is empty.
    ///
    /// This is the same as [Default::default].
    pub fn take() -> Self {
        Self(ManuallyDrop::new(take()))
    }

    /// Take an object from the pool with custom pool sizes.
    ///
    /// Creates a new object if the pool is empty. Configures the pool sizes if not already set.
    pub fn take_sz(max: usize, max_elements: usize) -> Self {
        Self(ManuallyDrop::new(take_sz(max, max_elements)))
    }

    /// Detach the object from the pool, returning the inner value.
    ///
    /// The detached object will not be returned to the pool when dropped.
    pub fn detach(self) -> T {
        // Don't drop Self and extract the inner type
        let t = ManuallyDrop::new(self);
        ManuallyDrop::into_inner(unsafe { ptr::read(&t.0) })
    }
}

impl<T: IsoPoolable> From<T> for LPooled<T> {
    fn from(t: T) -> Self {
        Self(ManuallyDrop::new(t))
    }
}

impl<T: IsoPoolable> AsRef<T> for LPooled<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T: IsoPoolable> Deref for LPooled<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: IsoPoolable> DerefMut for LPooled<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: IsoPoolable> Drop for LPooled<T> {
    fn drop(&mut self) {
        if self.really_dropped() {
            if let Some(t) = insert(unsafe { ptr::read(&*self.0) }) {
                drop(t)
            }
        } else {
            unsafe {
                ManuallyDrop::drop(&mut self.0);
            }
        }
    }
}

#[cfg(feature = "serde")]
impl<T: IsoPoolable + Serialize> Serialize for LPooled<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: IsoPoolable + DeserializeOwned + 'static> Deserialize<'de> for LPooled<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut t = LPooled::take();
        Self::deserialize_in_place(deserializer, &mut t)?;
        Ok(t)
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <T as Deserialize>::deserialize_in_place(deserializer, &mut place.0)
    }
}

impl<T: IsoPoolable + Extend<E>, E> Extend<E> for LPooled<T> {
    fn extend<I: IntoIterator<Item = E>>(&mut self, iter: I) {
        self.0.extend(iter)
    }
}

impl<T: IsoPoolable + Extend<E>, E> FromIterator<E> for LPooled<T> {
    fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
        let mut t = Self::take();
        t.extend(iter);
        t
    }
}
