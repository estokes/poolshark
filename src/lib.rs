//! Recycle expensive to construct/destruct objects
//!
//! Memory pooling is a simple idea, malloc and free can be expensive and so can
//! object initialization, if you malloc and init an object that is used
//! temporarially, and it's likely you'll want to use a similar object again in
//! the future, then don't throw all that work away by freeing it, stick it
//! somewhere until you need it again.
//!
//! There are two different types of pools implemented by this library, global
//! pools and local pools. Global pools share objects between threads (see
//! [global::GPooled]), an object taken from a global pool will always return to
//! the pool it was taken from. Use this if objects are usually dropped on a
//! different thread than they are created on, for example a producer thread
//! creating objects for consumer threads. There are several different ways to
//! use global pools. You can use [global::take] or [global::take_any] to just
//! take objects from thread local global pools. If you need better performance
//! you can use [global::pool] or [global::pool_any] and then store the pool
//! somewhere. If you don't have anywhere to store the pool you can use a static
//! [std::sync::LazyLock] for a truly global named pool. For example,
//!
//! ```no_run
//! use std::{sync::LazyLock, collections::HashMap};
//! use poolshark::global::{Pool, GPooled};
//!
//! type Widget = HashMap<usize, usize>;
//!
//! // create a global static widget pool that will accept up to 1024 widgets with
//! // up to 64 elements of capacity
//! static WIDGETS: LazyLock<Pool<Widget>> = LazyLock::new(|| Pool::new(1024, 64));
//!
//! fn widget_maker() -> GPooled<Widget> {
//!     let mut w = WIDGETS.take();
//!     w.insert(42, 42);
//!     w
//! }
//!
//! fn widget_user(w: GPooled<Widget>) {
//!     drop(w) // puts the widget back in the WIDGETS pool
//! }
//! ```
//!
//! Local pools (see [local::LPooled]) always return dropped objects to a thread
//! local structure on the thread that drops them. If your objects are produced
//! and dropped on the same set of threads then a local pool is a good choice.
//! Local pools are significantly faster than global pools because they avoid
//! most atomic operations. Local pools require that your container type
//! implement the unsafe trait IsoPoolable, so they can't be used with all
//! types. When they can be used they are quite easy,
//!
//! ```no_run
//! use poolshark::local::LPooled;
//! use std::collections::HashMap;
//!
//! type Widget = HashMap<usize, usize>;
//!
//! fn widget_maker() -> LPooled<Widget> {
//!     let mut w = LPooled::<Widget>::default(); // takes from the local pool
//!     w.insert(42, 42);
//!     w
//! }
//!
//! fn widget_user(w: LPooled<Widget>) {
//!     drop(w) // puts the widget back in the local pool
//! }
//! ```
use global::WeakPool;
pub use poolshark_derive::location_id;
use std::alloc::Layout;

pub mod global;
pub mod local;
pub mod pooled;

/// This is the unique id of a location in the code. use the
/// poolshark_derive::location_id!() macro to generate one
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocationId(pub u16);

#[cfg(test)]
mod test;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ULayout(u16);

impl Default for ULayout {
    fn default() -> Self {
        Self(0)
    }
}

impl ULayout {
    const fn empty() -> Self {
        Self(0)
    }

    const fn new<T>() -> Option<Self> {
        let l = Layout::new::<T>();
        let size = l.size();
        let align = l.align();
        if size >= 0x0FFF {
            return None;
        }
        if align > 0x10 {
            return None;
        }
        Some(Self(((size << 4) | (0x0F & align)) as u16))
    }

    const fn new_size<const SIZE: usize>() -> Option<Self> {
        // slight abuse of ULayout ...
        if SIZE > 0xFFFF {
            return None;
        }
        Some(ULayout(SIZE as u16))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Discriminant {
    container: LocationId,
    elements: [ULayout; 3],
}

impl Discriminant {
    pub const fn new(id: LocationId) -> Option<Discriminant> {
        Some(Discriminant {
            container: id,
            elements: [ULayout::empty(); 3],
        })
    }

    pub const fn new_p1<T>(id: LocationId) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 3];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub const fn new_p1_size<T, const SIZE: usize>(id: LocationId) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 3];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        match ULayout::new_size::<SIZE>() {
            Some(l) => elements[1] = l,
            None => return None,
        }
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub const fn new_p2<T, U>(id: LocationId) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 3];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        match ULayout::new::<U>() {
            Some(l) => elements[1] = l,
            None => return None,
        }
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub const fn new_p2_size<T, U, const SIZE: usize>(id: LocationId) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 3];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        match ULayout::new::<U>() {
            Some(l) => elements[1] = l,
            None => return None,
        }
        match ULayout::new_size::<SIZE>() {
            Some(l) => elements[2] = l,
            None => return None,
        }
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub const fn new_p3<T, U, V>(id: LocationId) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 3];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        match ULayout::new::<U>() {
            Some(l) => elements[1] = l,
            None => return None,
        }
        match ULayout::new::<V>() {
            Some(l) => elements[2] = l,
            None => return None,
        }
        Some(Discriminant {
            container: id,
            elements,
        })
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

/// Trait for poolable objects
pub trait Poolable {
    /// allocate a new empty collection
    fn empty() -> Self;

    /// empty the collection and reset it to it's default state so it
    /// can be put back in the pool. This will be called when the
    /// Pooled wrapper has been dropped and the object is being put
    /// back in the pool.
    fn reset(&mut self);

    /// return the capacity of the collection
    fn capacity(&self) -> usize;

    /// return true if the object has really been dropped, e.g. if
    /// you're pooling an Arc then Arc::get_mut().is_some() == true.
    fn really_dropped(&mut self) -> bool {
        true
    }
}

/// Implementing this trait allows full low level control over where
/// the pool pointer is stored. For example if you are pooling an
/// allocated data structure, you could store the pool pointer in the
/// allocation to keep the size of the handle struct to a
/// minimum. E.G. you're pooling a ThinArc. Or, if you have a static
/// global pool, then you would not need to keep a pool pointer at
/// all.
///
/// The object's drop implementation should return the object to the
/// pool instead of deallocating it
///
/// Implementing this trait correctly is extremely tricky, and
/// requires unsafe code in almost all cases, therefore it is marked
/// as unsafe
///
/// Most of the time you should use the `Pooled` wrapper as it's
/// required trait is much eaiser to implement and there is no
/// practial place to put the pool pointer besides on the stack.
pub unsafe trait RawPoolable: Sized {
    /// allocate a new empty object and set it's pool pointer to `pool`
    fn empty(pool: WeakPool<Self>) -> Self;

    /// empty the collection and reset it to it's default state so it
    /// can be put back in the pool
    fn reset(&mut self);

    /// return the capacity of the collection
    fn capacity(&self) -> usize;

    /// Actually drop the inner object, don't put it back in the pool,
    /// make sure you do not call both this method and the drop
    /// implementation that puts the object back in the pool!
    fn really_drop(self);
}

/// Trait for isomorphicly poolable objects. That is objects that can safely be
/// pooled by memory layout and alignment.
pub unsafe trait IsoPoolable: Poolable {
    /// Build a discriminant for Self. The discriminant container id must be
    /// unique for the container type, for example, Vec must always have a
    /// different container id from HashMap. You can use the macro
    /// `container_id_once` to ensure this with low overhead.
    ///
    /// # Getting the Layout Right
    ///
    /// You must pass every type variable that can effect the layout of the
    /// container's inner allocation to Discriminant. Take HashMap as an
    /// example. If you build the discriminant from HashMap<K, V> it will always
    /// be the same for any K, V, because the HashMap struct doesn't actually
    /// contain K and V, just a pointer to a collection of K, V. If you
    /// implemented discriminant this way it would cause your program to crash
    /// when you tried to pool two HashMap's with K, V types that aren't
    /// isomorphic. Instead you must pass K and V to Discriminant::new_p2::<K,
    /// V>() to get the real layout of the inner type of HashMap. This is why
    /// this trait is unsafe to implement, if you aren't careful when you build
    /// the discriminant very bad things will happen.
    ///
    /// # Why not TypeId
    ///
    /// The reason why Discriminant is used instead of TypeId (which would
    /// accomplish the same goal) is twofold. First Discriminant is 1 word on a
    /// 64 bit machine, and thus very fast to index, and second TypeId only
    /// supports types without references. However we often want to pool empty
    /// containers where the inner type is a reference, thus we cannot use TypeId.
    ///
    /// # Why Return Option
    ///
    /// Discriminant is a compressed version of layout that squeezes 3 layouts
    /// and a container type into 8 bytes. As such there are some layouts that
    /// are too big to fit in it, and the constructor will return None in those
    /// cases. For example if you want to pool a Vec<T> and T's size is greater
    /// then 0x0FFF (4K) then Discriminant will return None, also if your
    /// alignment is greater then 0xF (16) Discriminant will return None. For
    /// the purpose of pooling containers of small objects these tradeoffs
    /// seemed worth it. if you must pool containers of huge objects like this,
    /// you can use the thread safe pools.
    ///
    /// # Arc
    ///
    /// It is not safe to implement this trait in general for something like
    /// Arc, or any container that can't be totally empty (like array). This is
    /// because having the same Discriminant only guarantees that two types are
    /// isomorphic, it does not guarantee that they have the same bit patterns.
    /// Normal container types are safe in spite of this because reset makes
    /// sure they are empty, and thus no errent bit patterns exist in the
    /// container and all we care about is that the container's allocation is
    /// isomorphic with respect to the types we want to put in it.
    const DISCRIMINANT: Option<Discriminant>;
}
