//! Thread local pools with minimal atomic operations. This is faster than the
//! cross thread shared pool, at the cost of the following differences,
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

use crate::{Discriminant, LocalPoolable};
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

struct Pool<T: LocalPoolable> {
    max: usize,
    max_capacity: usize,
    data: Vec<T>,
}

impl<T: LocalPoolable> Pool<T> {
    fn new(max: usize, max_capacity: usize) -> Self {
        Self {
            max,
            max_capacity,
            data: Vec::with_capacity(max),
        }
    }
}

struct Opaque {
    t: *mut (),
    drop: Option<Box<dyn FnOnce(*mut ())>>,
}

impl Drop for Opaque {
    fn drop(&mut self) {
        if let Some(f) = self.drop.take() {
            f(self.t)
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
// 1. Chunks are reset before being returned to pools, so they contain no active K or V values
// 2. We only reuse pools for types with identical memory layouts (same size/alignment via Discriminant)
// 3. The Opaque wrapper ensures proper cleanup when the thread local is destroyed
fn with_pool<T, R, F>(f: F) -> R
where
    T: LocalPoolable,
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
                    let (size, cap) = SIZES
                        .lock()
                        .unwrap()
                        .get(&d)
                        .map(|(s, c)| (*s, *c))
                        .unwrap_or(DEFAULT_SIZES);
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
pub fn clear_type<T: LocalPoolable>() {
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
pub fn set_size<T: LocalPoolable>(max_pool_size: usize, max_element_capacity: usize) {
    if let Some(d) = T::DISCRIMINANT {
        SIZES
            .lock()
            .unwrap()
            .insert(d, (max_pool_size, max_element_capacity));
    }
}

/// get the max pool size and the max element capacity for a given type. If
/// get_size returns None then the type will not be pooled.
pub fn get_size<T: LocalPoolable>() -> Option<(usize, usize)> {
    T::DISCRIMINANT.map(|d| {
        SIZES
            .lock()
            .unwrap()
            .get(&d)
            .map(|(s, c)| (*s, *c))
            .unwrap_or(DEFAULT_SIZES)
    })
}

/// Take a T from the pool, if there is no pool for T or there are no Ts pooled
/// then create a new empty T
pub fn take<T: LocalPoolable>() -> T {
    with_pool(|pool| pool.and_then(|p| p.data.pop())).unwrap_or_else(|| T::empty())
}

/// Insert a T into the pool. If there is no space in the pool available to hold
/// T then return it, otherwise return None. Do not reset T, the caller is
/// responsible for resetting T. If you do not, horrible things can happen.
pub unsafe fn insert_raw<T: LocalPoolable>(t: T) -> Option<T> {
    with_pool(|pool| match pool {
        Some(pool) if pool.data.len() < pool.max && t.capacity() <= pool.max_capacity => {
            pool.data.push(t);
            None
        }
        None | Some(_) => Some(t),
    })
}

/// Insert a T into the pool. If there is no space in the pool available to hold
/// T then return it, otherwise return None. T will be reset before it is
/// inserted into the pool. Reset must ensure that T is EMPTY.
pub fn insert<T: LocalPoolable>(mut t: T) -> Option<T> {
    t.reset();
    unsafe { insert_raw(t) }
}

/// A generic wrapper around locally pooled objects that manages Drop for you.
/// If you have implemented LocalPooled on your object, you can just wrap it in
/// a Pooled<T> and you should be done. Unlink global::Pooled<T> this object is
/// zero size.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pooled<T: LocalPoolable>(ManuallyDrop<T>);

impl<T: LocalPoolable> Borrow<T> for Pooled<T> {
    fn borrow(&self) -> &T {
        &self.0
    }
}

impl Borrow<str> for Pooled<String> {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<T: LocalPoolable> Pooled<T> {
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

impl<T: LocalPoolable> From<T> for Pooled<T> {
    fn from(t: T) -> Self {
        Self(ManuallyDrop::new(t))
    }
}

impl<T: LocalPoolable> AsRef<T> for Pooled<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T: LocalPoolable> Deref for Pooled<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: LocalPoolable> DerefMut for Pooled<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: LocalPoolable> Drop for Pooled<T> {
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
impl<T: LocalPoolable + Serialize> Serialize for Pooled<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: LocalPoolable + DeserializeOwned + 'static> Deserialize<'de> for Pooled<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut t = Pooled::take();
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
