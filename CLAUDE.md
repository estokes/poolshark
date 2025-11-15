# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Poolshark is a thread-safe object pool library for Rust that minimizes memory allocations by reusing objects. It supports two pooling strategies:

- **Local Pools** (`local::LPooled`): Fast, thread-local pooling where objects return to the pool of the thread that drops them. Use when objects stay on the same thread or are evenly distributed.
- **Global Pools** (`global::GPooled`): Lock-free, thread-safe pools where objects always return to their originating pool. Use for producer-consumer patterns across threads.

## Common Development Commands

### Building and Testing
```bash
# Build the library
cargo build

# Build with all features
cargo build --all-features

# Run tests
cargo test

# Run a specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Check for compilation errors
cargo check
```

### Running Examples
```bash
# Run the local pool example
cargo run --example local

# Run the global pool example
cargo run --example global
```

### Working with the Derive Crate
The `poolshark_derive` subdirectory contains a proc macro for generating unique location IDs:

```bash
# Build just the derive crate
cargo build -p poolshark_derive

# Test the derive crate
cargo test -p poolshark_derive
```

### Documentation
```bash
# Build and open documentation
cargo doc --open

# Build docs with all features
cargo doc --all-features --open
```

### Publishing
```bash
# Verify the package before publishing
cargo package --list

# Publish to crates.io (requires authentication)
cargo publish
```

## Architecture

### Core Traits

**Poolable** (`src/lib.rs:256-273`): Base trait for all poolable types. Defines:
- `empty()`: Create new empty instance
- `reset(&mut self)`: Clear contents for reuse
- `capacity(&self)`: Return current capacity
- `really_dropped(&mut self) -> bool`: Check if object is truly dropped (important for Arc-like types)

**IsoPoolable** (`src/lib.rs:314-358`): Unsafe trait for local pooling via isomorphic type reuse. Key constraint: types must be reusable based on memory layout alone (e.g., `HashMap<K,V>` where different K,V pairs can share allocations when empty). Requires a `DISCRIMINANT` that encodes container type and type parameter layouts.

**RawPoolable** (`src/lib.rs:291-306`): Low-level trait for global pools with manual pool pointer management. Used internally by `GPooled`.

### Discriminant System

`Discriminant` (`src/lib.rs:148-241`) enables safe local pooling by encoding in 8 bytes:
- Container type location ID (via `location_id!()` macro)
- Layouts of up to 2 type parameters
- Optional const SIZE parameter

Constraints:
- Max 0xFFFF `IsoPoolable` implementations per project
- Type parameters: size ≤ 0x0FFF bytes, alignment ≤ 0xF
- Const SIZE < 0xFFFF

If constraints are violated, constructors return `None` and pooling is disabled (objects allocated/freed normally).

### Location ID Generation

The `location_id!()` proc macro (`poolshark_derive/src/lib.rs`) generates globally unique IDs per source code position by:
1. Extracting call site location (crate, file, line, column)
2. Storing allocations in `<OUT_DIR>/.poolshark_loc_ids`
3. Maintaining a persistent BTreeMap across compilations

This enables safe cross-crate type discrimination without TypeId (which doesn't support references).

### Local Pools (`src/local/mod.rs`)

Thread-local pools stored in `POOLS: RefCell<FxHashMap<Discriminant, Opaque>>`. Each thread maintains separate pools per discriminant.

Key functions:
- `take<T>()` / `take_sz()`: Get object from pool or create new
- `insert<T>()` / `insert_raw<T>()`: Return object to pool (insert calls reset first)
- `set_size<T>()`: Configure max pool size and max element capacity
- `clear()` / `clear_type<T>()`: Empty pools

`LPooled<T>` wrapper manages drop automatically. Objects can be sent between threads but return to the pool of the dropping thread.

### Global Pools (`src/global/mod.rs`)

Lock-free pools using `crossbeam_queue::ArrayQueue`. Objects maintain a `WeakPool` pointer and return to their origin pool.

Pool types:
- `RawPool<T: RawPoolable>`: Generic pool for types that manage their own pool pointer
- `Pool<T>` = `RawPool<GPooled<T>>`: Convenience alias for pooled containers

Key functions:
- `take<T>()` / `take_sz()`: Get from thread-local global pool instance
- `pool<T>()` / `pool_sz<T>()`: Get shareable pool reference (for IsoPoolable types)
- `pool_any<T>()` / `take_any<T>()`: Use TypeId-based pools (for Any + Poolable types)

`GPooled<T>` stores `WeakPool` pointer (1 word overhead) and implements drop to return to origin.

### Poolable Implementations (`src/pooled.rs`)

Standard types with `Poolable` + `IsoPoolable`:
- `Vec<T>`, `VecDeque<T>`, `String`
- `HashMap<K,V>`, `HashSet<K>` (with hasher constraint)
- `IndexMap<K,V>`, `IndexSet<K>` (feature gated)
- `Option<T: Poolable>`

Discriminants use `location_id!()` with appropriate type parameters (e.g., `Vec<T>` uses `new_p1::<T>`, `HashMap` uses `new_p2::<K,V>`).

### Arc Pooling (`src/global/arc.rs`)

Specialized poolable Arc implementations that embed pool pointers in the allocation:
- `Arc<T>`: Drop-in replacement for `std::sync::Arc` with pooling
- `TArc<T>`: Uses `triomphe::Arc` internally (lighter weight)

These implement `RawPoolable` and only return to pool when strong_count == 1.

## Features

- `default = ["triomphe", "indexmap", "serde"]`
- `triomphe`: Enable `TArc<T>` poolable Arc
- `indexmap`: Enable pooling for `IndexMap` and `IndexSet`
- `serde`: Implement Serialize/Deserialize for `LPooled` and `GPooled`

## Testing Strategy

Tests in `src/test.rs` verify memory safety with valgrind comments showing zero leaks:
- `normal_pool`: Global pool correctness and pointer reuse
- `local_pool`: Local pool type discrimination (different type params get different pools)
- `tarc_pool` / `arc_pool`: Arc refcounting integration

Run memory checks with:
```bash
valgrind --leak-check=full cargo test
```

## Important Implementation Details

1. **Reset safety**: `reset()` must completely empty containers. Discriminant only guarantees layout compatibility, not bit pattern compatibility.

2. **Recursive drop protection**: Pool access uses `try_borrow_mut()` to handle cases where drop implementations try to reenter the pool.

3. **Thread safety**: `LPooled` is Send+Sync even though pools are thread-local. Objects can migrate between threads; they just return to the dropping thread's pool.

4. **Capacity limits**: Pools have `max_pool_size` (max pooled objects) and `max_element_capacity` (max object size). Objects exceeding limits are deallocated.

5. **Orphans**: `GPooled::orphan(t)` creates unpooled objects (useful for known-empty cases). Can be assigned to a pool later with `assign()`.
