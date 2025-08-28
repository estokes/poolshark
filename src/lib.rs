use std::{
    alloc::Layout,
    sync::atomic::{AtomicU16, Ordering},
};

pub mod global;
pub mod local;
pub mod pooled;

#[cfg(test)]
mod test;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerId(u16);

impl ContainerId {
    pub fn new() -> Self {
        static NEXT: AtomicU16 = AtomicU16::new(16);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        if id < 16 {
            panic!("too many container implementations")
        }
        Self(id)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ULayout(u16);

impl Default for ULayout {
    fn default() -> Self {
        Self(0)
    }
}

impl ULayout {
    fn new<T>() -> Option<Self> {
        let l = Layout::new::<T>();
        let size = l.size();
        let align = l.align();
        if size >= 0x0FFF {
            return None;
        }
        if align > 0x0F {
            return None;
        }
        Some(Self(((size << 4) | (0x0F & align)) as u16))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Discriminant {
    container: ContainerId,
    elements: [ULayout; 3],
}

impl Discriminant {
    pub fn new(id: ContainerId) -> Option<Discriminant> {
        Some(Discriminant {
            container: id,
            elements: [ULayout::default(); 3],
        })
    }

    pub fn new_p1<T>(id: ContainerId) -> Option<Discriminant> {
        let mut elements = [ULayout::default(); 3];
        elements[0] = ULayout::new::<T>()?;
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub fn new_p2<T, U>(id: ContainerId) -> Option<Discriminant> {
        let mut elements = [ULayout::default(); 3];
        elements[0] = ULayout::new::<T>()?;
        elements[1] = ULayout::new::<U>()?;
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub fn new_p3<T, U, V>(id: ContainerId) -> Option<Discriminant> {
        let mut elements = [ULayout::default(); 3];
        elements[0] = ULayout::new::<T>()?;
        elements[1] = ULayout::new::<U>()?;
        elements[1] = ULayout::new::<V>()?;
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

/// Trait for thread local poolable objects
pub unsafe trait LocalPoolable: Poolable {
    /// Build a discriminant for Self. The discriminant container id must be
    /// unique for the container type, for example, Vec must always have a
    /// different container id from HashMap. You must pass every type variable
    /// that can effect the layout of the type to Discriminant
    ///
    /// It is not safe to implement this trait in general for something like
    /// Arc, or any container that can't be totally empty (like array). This is
    /// because having the same Discriminant only guarantees that two types are
    /// isomorphic, it does not guarantee that they have the same bit patterns.
    ///
    /// If you do this wrong, you WILL cause crashes, or worse
    fn discriminant() -> Option<Discriminant>;
}
