use std::{alloc::Layout, cell::Cell};

pub mod global;
pub mod local;
pub mod pooled;

#[cfg(test)]
mod test;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerId(u16);

impl ContainerId {
    /// Will never be used for an id, Discriminant will reject it, so you can
    /// use it as a default value in a thread local Cell to init your own ids
    pub const INVALID: ContainerId = ContainerId(0);

    pub fn new() -> Self {
        thread_local! {
            static NEXT: Cell<u16> = Cell::new(16);
        }
        let id = NEXT.get();
        if id < 16 {
            panic!("too many container implementations")
        }
        NEXT.set(id + 1);
        Self(id)
    }
}

/// Get a unique container id for a given macro site for a given thread. This
/// will generate a unique container id the first time, and then will always
/// return the same id with low overhead.
#[macro_export]
macro_rules! container_id_once {
    () => {{
        thread_local! {
            static ID: ::core::cell::Cell<$crate::ContainerId> =
                ::core::cell::Cell::new($crate::ContainerId::INVALID);
        }
        let id = ID.get();
        if id != $crate::ContainerId::INVALID {
            id
        } else {
            ID.set($crate::ContainerId::new());
            ID.get()
        }
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        if align > 0x10 {
            return None;
        }
        Some(Self(((size << 4) | (0x0F & align)) as u16))
    }

    fn new_size<const SIZE: usize>() -> Option<Self> {
        // slight abuse of ULayout ...
        if SIZE > 0xFFFF {
            return None;
        }
        Some(ULayout(SIZE as u16))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Discriminant {
    container: ContainerId,
    elements: [ULayout; 3],
}

impl Discriminant {
    pub fn new(id: ContainerId) -> Option<Discriminant> {
        if id == ContainerId::INVALID {
            return None;
        }
        Some(Discriminant {
            container: id,
            elements: [ULayout::default(); 3],
        })
    }

    pub fn new_p1<T>(id: ContainerId) -> Option<Discriminant> {
        if id == ContainerId::INVALID {
            eprintln!("invalid id");
            return None;
        }
        let mut elements = [ULayout::default(); 3];
        elements[0] = ULayout::new::<T>()?;
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub fn new_p1_size<T, const SIZE: usize>(id: ContainerId) -> Option<Discriminant> {
        if id == ContainerId::INVALID {
            eprintln!("invalid id");
            return None;
        }
        let mut elements = [ULayout::default(); 3];
        elements[0] = ULayout::new::<T>()?;
        elements[1] = ULayout::new_size::<SIZE>()?;
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub fn new_p2<T, U>(id: ContainerId) -> Option<Discriminant> {
        if id == ContainerId::INVALID {
            return None;
        }
        let mut elements = [ULayout::default(); 3];
        elements[0] = ULayout::new::<T>()?;
        elements[1] = ULayout::new::<U>()?;
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub fn new_p2_size<T, U, const SIZE: usize>(id: ContainerId) -> Option<Discriminant> {
        if id == ContainerId::INVALID {
            return None;
        }
        let mut elements = [ULayout::default(); 3];
        elements[0] = ULayout::new::<T>()?;
        elements[1] = ULayout::new::<U>()?;
        elements[2] = ULayout::new_size::<SIZE>()?;
        Some(Discriminant {
            container: id,
            elements,
        })
    }

    pub fn new_p3<T, U, V>(id: ContainerId) -> Option<Discriminant> {
        if id == ContainerId::INVALID {
            return None;
        }
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
