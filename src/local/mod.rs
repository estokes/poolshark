//! Thread local object pools
//!
//! This is faster than the cross thread shared pool, at the cost of the
//! following differences,
//!
//! - more memory may be used as pools are thread local, you cannot centrally
//! share pooled objects
//!
//! - an extra unsafe trait to implement
//!
//! - if an element is dropped on a different thread than it was allocated on
//! then it will be returned to a different pool
//!
//! Still this is about as close as it gets to having your cake and also eating
//! it. You get to pool objects with minimal atomics without making all your
//! pooled objects !Send (which is what would happen if you tried to directly use a Vec).

use crate::{Discriminant, IsoPoolable, Opaque};
use fxhash::FxHashMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    borrow::Borrow,
    cell::RefCell,
    collections::HashMap,
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
        Self {
            max,
            max_capacity,
            data: Vec::with_capacity(max),
        }
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

/// Clear all thread local pools on this thread. Note this will happen
/// automatically when the thread dies.
pub fn clear() {
    POOLS.with_borrow_mut(|pools| pools.clear())
}

/// Delete the thread local pool for the specified K, V and SIZE. This will
/// happen automatically when the current thread dies.
pub fn clear_type<T: IsoPoolable>() {
    POOLS.with_borrow_mut(|pools| {
        if let Some(d) = T::DISCRIMINANT {
            pools.remove(&d);
        }
    })
}

/// Set the pool size for this type. Pools that have already been created will
/// not be resized, but new pools (on new threads) will use the specified size
/// as their max size. If you wish to resize an existing pool you can first
/// clear_type (or clear) and then set_size.
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

fn take_inner<T: IsoPoolable>(sizes: Option<(usize, usize)>) -> T {
    with_pool(sizes, |pool| pool.and_then(|p| p.data.pop())).unwrap_or_else(|| T::empty())
}

/// Take a T from the pool, if there is no pool for T or there are no Ts pooled
/// then create a new empty T
pub fn take<T: IsoPoolable>() -> T {
    take_inner(None)
}

/// Take a T from the pool, if there is no pool for T or there are no Ts pooled
/// then create a new empty T. Configure the max size and max_elt size of the
/// pool if it has not already been created.
pub fn take_sz<T: IsoPoolable>(max: usize, max_elt: usize) -> T {
    take_inner(Some((max, max_elt)))
}

unsafe fn insert_raw_inner<T: IsoPoolable>(sizes: Option<(usize, usize)>, t: T) -> Option<T> {
    with_pool(sizes, |pool| match pool {
        Some(pool) if pool.data.len() < pool.max && t.capacity() <= pool.max_capacity => {
            pool.data.push(t);
            None
        }
        None | Some(_) => Some(t),
    })
}

/// Insert a T into the pool. If there is no space in the pool available to hold
/// T then return it, otherwise return None. Does not reset T, the caller is
/// responsible for resetting T. If you do not, horrible things can happen.
pub unsafe fn insert_raw<T: IsoPoolable>(t: T) -> Option<T> {
    unsafe { insert_raw_inner(None, t) }
}

/// Insert a T into the pool. If there is no space in the pool available to hold
/// T then return it, otherwise return None. Does not reset T, the caller is
/// responsible for resetting T. If you do not, horrible things can happen. Also
/// set the max pool size and max_elt size if the pool has not been initialized
/// yet.
pub unsafe fn insert_raw_sz<T: IsoPoolable>(max: usize, max_elt: usize, t: T) -> Option<T> {
    unsafe { insert_raw_inner(Some((max, max_elt)), t) }
}

/// Insert a T into the pool. If there is no space in the pool available to hold
/// T then return it, otherwise return None. T will be reset before it is
/// inserted into the pool. Reset must ensure that T is EMPTY.
pub fn insert<T: IsoPoolable>(mut t: T) -> Option<T> {
    t.reset();
    unsafe { insert_raw(t) }
}

/// Insert a T into the pool. If there is no space in the pool available to hold
/// T then return it, otherwise return None. T will be reset before it is
/// inserted into the pool. Reset must ensure that T is EMPTY.
pub fn insert_sz<T: IsoPoolable>(max: usize, max_elt: usize, mut t: T) -> Option<T> {
    t.reset();
    unsafe { insert_raw_inner(Some((max, max_elt)), t) }
}

/// A zero size wrapper around locally pooled objects that manages Drop for you.
/// an `LPooled` object will be returned to the thread local pool on whatever
/// thread it is dropped. In most cases this is fine, but in specific cases
/// (e.g. producer consumer patterns) using a [GPooled](crate::global::GPooled) will be
/// more efficient.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LPooled<T: IsoPoolable>(ManuallyDrop<T>);

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
    /// take an object from the pool, or create one if the pool is empty. This
    /// is the same as [Default::default]
    pub fn take() -> Self {
        Self(ManuallyDrop::new(take()))
    }

    /// detach the object from the pool, returning it. the detached object will
    /// not be returned to the pool when dropped. If you later decide you'd like
    /// to reverse this decision you can call Pooled::from on T.
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
