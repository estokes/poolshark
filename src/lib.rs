//! Recycle expensive to construct/destruct objects
//!
//! There are three different ways of using this library.
//!
//! - Implement the safe trait Poolable for your type. Wrap everything you want
//! to pool in Pooled, and build static pools. This is the easiest and the
//! safest, but the performance gains are not as great. Your types get bigger
//! because they keep the pool pointer. Atomic operations create some overhead
//! when adding and removing to pools.
//!
//! - Implement RawPoolable directly. You can stash the pool pointer anywhere
//! (or nowhere) and implement other low level optimizations. This is not a safe
//! trait to implement, so don't do it unless you understand the rust memory
//! model well.
//!
//! - Implement LocalPoolable, and use thread local non atomic pools. This is
//! the fastest solution. It's about as difficult as RawPoolable. It's only
//! drawback is that you can't share pooled objects between threads, and so you
//! may end up wasting more memory.
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

/// Trait for thread local poolable objects
pub unsafe trait LocalPoolable: Poolable {
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
