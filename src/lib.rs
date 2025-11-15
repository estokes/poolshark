//! A high-performance object pool that reuses allocations instead of freeing them.
//!
//! # Quick Start
//!
//! ```
//! use poolshark::local::LPooled;
//! use std::collections::HashMap;
//!
//! // Take a HashMap from the thread-local pool (or create new if empty)
//! let mut map: LPooled<HashMap<String, i32>> = LPooled::take();
//! map.insert("answer".to_string(), 42);
//! // When dropped, the HashMap is cleared and returned to the pool
//! ```
//!
//! # Which Pool Should I Use?
//!
//! - **Use [`local::LPooled`]** (default choice): Faster, for objects created and dropped on the same thread(s)
//! - **Use [`global::GPooled`]**: When one thread creates objects and other threads drop them (producer-consumer)
//!
//! # Why Poolshark?
//!
//! - **Reduce allocations**: Reuse containers instead of repeatedly allocating and freeing
//! - **Predictable performance**: Consistent behavior across platforms, independent of allocator
//! - **Fast**: Local pools avoid atomic operations and are more ergonomic than `thread_local!`
//! - **Flexible**: Choose between fast thread-local pools or lock-free cross-thread pools
//!
//! # Pool Types
//!
//! ## Global Pooling
//!
//! Global pools share objects between threads (see [`global::GPooled`]).
//! An object taken from a global pool always returns to the pool it was
//! taken from, regardless of which thread drops it. Use this for producer-consumer
//! patterns where one thread creates objects and other threads consume them.
//!
//! There are several different ways to use global pools. You can use
//! [take](global::take) or [take_any](global::take_any) to just take objects
//! from thread local global pools. If you need better performance you can use
//! [pool](global::pool) or [pool_any](global::pool_any) and then store the pool
//! somewhere. If you don't have anywhere to store the pool you can use a static
//! [LazyLock](std::sync::LazyLock) for a truly global named pool. For example,
//!
//! ```no_run
//! use std::{sync::LazyLock, collections::HashMap};
//! use poolshark::global::{Pool, GPooled};
//!
//! type Widget = HashMap<usize, usize>;
//!
//! // create a global static widget pool that will accept up to 1024 widgets with
//! // up to 64 elements of capacity each
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
//! ## Local Pooling
//!
//! Local pools (see [`local::LPooled`]) always return dropped objects to a thread-local
//! pool on whichever thread drops them. They are significantly faster than global pools
//! because they avoid atomic operations. Use local pools by default unless you have
//! a cross-thread producer-consumer pattern.
//!
//! **Thread safety**: `LPooled<T>` is `Send + Sync` whenever `T` is `Send + Sync`, so you can
//! safely pass pooled objects between threads.
//!
//! Local pools require types to implement the unsafe trait [`IsoPoolable`], but all
//! standard containers (Vec, HashMap, String, etc.) already implement it.
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

/// A globally unique id for a source code position
///
/// use poolshark_derive::location_id!() macro to generate one
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
        if size > 0x0FFF {
            return None;
        }
        if align > 0x10 {
            return None;
        }
        Some(Self(((size << 4) | (0x0F & align)) as u16))
    }
}

/// Type describing the layout, alignment, and type of a container
///
/// `Discriminant` is central to the safety and performance of local pooling. It
/// describes 2 things in just 8 bytes.
///
/// - The unique location in the source code of the implementation of
/// [IsoPoolable]. This is accomplished by a proc macro that generates a global
/// table of unique location ids for cross crate source code locations. This
/// unique id ensures that different container types can't be mixed in the same
/// pool.
///
/// - The layout and alignment of all the type parameters of the container, up
/// to 2 are supported. If your container has more that two type parameters
/// then you can't locally pool it, and you can't implement [IsoPoolable]. If you
/// do, you will cause undefined behavior.
///
/// In order to squeeze all this information into just 8 bytes there are some
/// limitations.
///
/// - You can't have more than 0xFFFF implementations of [IsoPoolable] in the
/// same project. This includes all the crates depended on by the project.
///
/// - Your type parameters must have size <= 0x0FFF bytes and alignment <= 0xF.
///
/// - const SIZE parameters must be < 0xFFFF.
///
/// If any of these constraints are violated the `Discriminant` constructors
/// will return `None`. If you desire you may panic at that point to cause a
/// compile error. If you do not panic and instead leave `DISCRIMINANT` as
/// `None` then local pool operations on that type will work just fine, but
/// nothing will be pooled. Objects will be freed when they are dropped and
/// [take](local::take) will allocate new objects each time it is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Discriminant {
    container: LocationId,
    elements: [ULayout; 2],
    size: u16,
}

impl Discriminant {
    const NO_SIZE: u16 = 0xFFFF;

    /// build a discriminant for a type with no type variables (just a location
    /// id). Always returns Some
    pub const fn new(id: LocationId) -> Option<Discriminant> {
        Some(Discriminant {
            container: id,
            elements: [ULayout::empty(); 2],
            size: Self::NO_SIZE,
        })
    }

    /// build a discriminant for a type with 1 type variable `T`. Return `None` if
    /// `T` is too large to fit.
    pub const fn new_p1<T>(id: LocationId) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 2];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        Some(Discriminant { container: id, elements, size: Self::NO_SIZE })
    }

    /// build a discriminant for a type with 1 type variable `T` and a const
    /// `SIZE`. Return `None` if `T` or `SIZE` are too large to fit.
    pub const fn new_p1_size<T, const SIZE: usize>(
        id: LocationId,
    ) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 2];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        if SIZE >= 0xFFFF {
            return None;
        }
        Some(Discriminant { container: id, elements, size: SIZE as u16 })
    }

    /// build a discriminant for a type with two type variables `T` and `U`.
    /// Return `None` if either `T` or `U` are too large to fit
    pub const fn new_p2<T, U>(id: LocationId) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 2];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        match ULayout::new::<U>() {
            Some(l) => elements[1] = l,
            None => return None,
        }
        Some(Discriminant { container: id, elements, size: Self::NO_SIZE })
    }

    /// build a discriminant for a type with two type variables `T` and `U` and
    /// a const SIZE. Return `None` if any of the parameters are too large to
    /// fit.
    pub const fn new_p2_size<T, U, const SIZE: usize>(
        id: LocationId,
    ) -> Option<Discriminant> {
        let mut elements = [ULayout::empty(); 2];
        match ULayout::new::<T>() {
            Some(l) => elements[0] = l,
            None => return None,
        }
        match ULayout::new::<U>() {
            Some(l) => elements[1] = l,
            None => return None,
        }
        if SIZE >= 0xFFFF {
            return None;
        }
        Some(Discriminant { container: id, elements, size: SIZE as u16 })
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
    /// can be put back in the pool.
    fn reset(&mut self);

    /// return the capacity of the collection
    fn capacity(&self) -> usize;

    /// return true if the object has really been dropped, e.g. if
    /// you're pooling an Arc then Arc::get_mut().is_some() == true.
    fn really_dropped(&mut self) -> bool {
        true
    }
}

/// Low level global pool trait for maximum control
///
/// Implementing this trait allows full low level control over where the pool
/// pointer is stored. For example if you are pooling an allocated data
/// structure, you could store the pool pointer in the allocation to keep the
/// size of the handle struct to a minimum. E.G. you're pooling a
/// [triomphe::ThinArc]. Or, if you have a static global pool, then you would
/// not need to keep a pool pointer at all.
///
/// The object's drop implementation should return the object to the
/// pool instead of deallocating it
///
/// Implementing this trait correctly is extremely tricky, and requires unsafe
/// code, therefore it is marked as unsafe.
///
/// Most of the time you should use the [GPooled](global::GPooled) wrapper.
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

/// Trait for isomorphicly poolable objects.
///
/// That is objects that can safely be pooled by memory layout and container
/// type. For example two `HashMap`s, `HashMap<usize, usize>` and
/// `HashMap<ArcStr, ArcStr>` are isomorphic, their memory allocations can be
/// used interchangably so long as they are empty.
pub unsafe trait IsoPoolable: Poolable {
    /// # Getting the Layout Right
    ///
    /// You must pass every type variable that can effect the layout
    /// of the container's inner allocation to Discriminant. Take
    /// HashMap as an example. If you build the discriminant such as
    /// `Discriminant::new_p1::<HashMap<K, V>>()` it would always be
    /// the same for any `K`, `V`, because the `HashMap` struct
    /// doesn't actually contain any `K`s or `V`s, just a pointer to
    /// some `K`s and `V`s. If you implemented discriminant this way
    /// it would cause undefined behavior when you tried to pool two
    /// HashMap's with `K`, `V` types that aren't isomorphic. Instead
    /// you must pass `K` and `V` to `Discriminant::new_p2::<K, V>()`
    /// to get the real layout of the inner collection of
    /// `HashMap`. This is why this trait is unsafe to implement, if
    /// you aren't careful when you build the discriminant very bad
    /// things will happen.
    ///
    /// # Why not TypeId
    ///
    /// The reason why Discriminant is used instead of
    /// [`TypeId`](std::any::TypeId) (which would accomplish the same
    /// goal) is twofold. First Discriminant is 1 word on a 64 bit
    /// machine, and thus very fast to index, and second `TypeId` only
    /// supports types without references. However we often want to
    /// pool empty containers where the inner type is a reference,
    /// thus we cannot use `TypeId`.
    ///
    /// # Why Discriminant is an Option
    ///
    /// Discriminant is a compressed version of layout that squeezes 2
    /// layouts a size and a container type into 8 bytes. As such
    /// there are some layouts that are too big to fit in it, and the
    /// constructor will return None in those cases. For the purpose
    /// of pooling containers of small objects these tradeoffs seemed
    /// worth it. If you must pool containers of huge objects like
    /// this, you can use the global pools.
    ///
    /// # Arc
    ///
    /// It is not safe to implement this trait for
    /// [`Arc`](std::sync::Arc) or in general for any container that
    /// can't be totally empty. This is because having the same
    /// Discriminant only guarantees that two types are isomorphic, it
    /// does not guarantee that they have the same bit patterns.
    /// Normal container types are safe in spite of this because reset
    /// makes sure they are empty, and thus no errent bit patterns
    /// exist in the container and all we care about is that the
    /// container's allocation is isomorphic with respect to the types
    /// we want to put in it. However `Arc` can never be empty, and
    /// since notch optimization may change the bit pattern of `None`
    /// depending on the type of `T`, it is not even safe to pool
    /// `Arc<Option<T>>`. Because if `T` and `U` were isomorphic, but
    /// notch optimization used a different bit pattern for `None`,
    /// then pooling these objects could cause undefined behavior.
    const DISCRIMINANT: Option<Discriminant>;
}
