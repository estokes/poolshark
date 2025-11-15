# Thread Safe Object Pool

A high-performance, general-purpose object pool that reuses
allocations instead of freeing them. Supports most standard library
containers (Vec, HashMap, String, etc.) plus external types like
IndexMap and triomphe::Arc.

## Why Poolshark?

- **Reduce allocations**: Reuse containers instead of repeatedly allocating and freeing
- **Predictable performance**: Consistent behavior across platforms, independent of allocator quality
- **Low-cost abstraction**: Local pools have performance similar to thread_local! with simpler ergonomics
- **Flexible**: Choose between fast thread-local pools or lock-free cross-thread pools

## Installation

```bash
cargo add poolshark
```

## Quick Start

```rust
use poolshark::local::LPooled;
use std::collections::HashMap;

// Take a HashMap from the thread-local pool (or create new if the pool is empty)
let mut map: LPooled<HashMap<String, i32>> = LPooled::take();
map.insert("answer".to_string(), 42);
// When dropped, the HashMap is cleared and returned to the pool
```

## Which Pool Should I Use?

| Use Local Pools (`LPooled`) when... | Use Global Pools (`GPooled`) when... |
|--------------------------------------|--------------------------------------|
| Objects are created and dropped on the same thread(s) | One thread creates objects, other threads drop them |
| You want maximum performance | You need objects to return to a specific pool |

**Rule of thumb**: Start with `LPooled` (faster). Switch to `GPooled`
only if you have cross-thread producer-consumer patterns.

## Local Pools

Local pools are thread-local but more ergonomic than
`thread_local!`. You can own the objects, pass them between threads,
and use them naturally. When dropped, objects return to the pool of
*whichever thread drops them*—not necessarily where they were created.

**Performance**: Faster than global pools due to no atomic operations,
not significantly different than `thread_local!`  (on which it is
based). Use these by default unless you have a cross-thread
producer-consumer pattern.

### Example: Deduplication With Minimal Allocations

```rust
use poolshark::local::LPooled;
use std::{collections::HashSet, hash::Hash};

// dedup an unsorted vec. this will only allocate memory on,
// - the first call on a given thread
// - if deduping a vec that is bigger than any previously seen
// - if deduping a vec that is bigger than the max length allowed in the pool
fn unsorted_dedup_stable<T: Hash + Eq>(v: &mut Vec<T>) {
    let mut set: LPooled<HashSet<&T>> = LPooled::take();
    let mut remove: LPooled<Vec<usize>> = LPooled::take();
    let mut removed = 0;
    for (i, t) in v.iter().enumerate() {
        if !set.insert(t) {
            remove.push(i - removed);
            removed += 1
        }
    }
    drop(set); // set is cleared and pushed to the thread local pool
    for i in remove.drain(..) {
        v.remove(i);
    }
    // remove is cleared and pushed to the thread local pool
}

fn main() {
    let mut v = vec!["one", "two", "one", "five", "three sir", "three", "four", "five"];
    println!("with dupes: {:?}", v);
    unsorted_dedup_stable(&mut v);
    println!("deduped: {:?}", v)
}
```

## Global Pools

Global pools use lock-free queues to ensure objects always return to
their origin pool, regardless of which thread drops them.

**Performance**: Will usually be faster than malloc/free. In cases
where it isn't, it's usually close. Consistent across platforms with
very different allocators.

### Example: Producer-Consumer Pattern

```rust
use poolshark::global::{GPooled, Pool};
use std::sync::LazyLock;
use tokio::{sync::mpsc, task};

// a batch is a vec of pooled strings
type Batch = Vec<GPooled<String>>;

// strings will come from this pool. it can hold 1024 strings up to 4k in size.
// any string bigger than 4k will be thrown away. After the pool is full newly
// returned strings will be thrown away. This bounds the memory that can be
// consumed by this pool, but doesn't limit the number of strings that can exist.
static STRINGS: LazyLock<Pool<String>> = LazyLock::new(|| Pool::new(1024, 4096));

// batches will come from this pool, which can hold 1024 batches of up to 1024 elements
// in size.
static BATCHES: LazyLock<Pool<Batch>> = LazyLock::new(|| Pool::new(1024, 1024));

async fn producer(tx: mpsc::Sender<GPooled<Batch>>) {
    use std::fmt::Write;
    loop {
        // take a batch from the pool. if the pool is empty a new
        // batch will be allocated.
        let mut batch = BATCHES.take();
        for _ in 0..100 {
            // take a new string from the pool. if the pool is empty a new string
            // will be allocated.
            let mut s = STRINGS.take();
            write!(s, "very important data").unwrap();
            batch.push(s)
        }
        if let Err(_) = tx.send(batch).await {
            break; // stop if the channel closes
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let (tx, mut rx) = mpsc::channel(10);
    task::spawn(producer(tx));
    while let Some(mut batch) = rx.recv().await {
        for s in batch.drain(..) {
            println!("a message from our sponsor {s}")
        }
        // s is dropped here. the string length is set to 0 and is
        // pushed on the STRINGS pool.
    }
    // batch dropped here. the vec is cleared and pushed on the BATCHES pool
}

// Once an initial working set is allocated this program does not call
// malloc again, and free is never called except before exit.

// Depending on the platform allocator this is usually faster than a
// constant churn of malloc/free ops. Whether or not it's faster on a
// particular platform, it is more deterministic across platforms. Yes
// the platform allocator may pull all the tricks in the book and
// might even perform better, but move to some other platform and
// performance is awful again.
```

## Supported Types

**Built-in support** (no additional code needed):
- `Vec<T>`, `VecDeque<T>`, `String`
- `HashMap<K, V>`, `HashSet<K>`
- `IndexMap<K, V>`, `IndexSet<K>` (with `indexmap` feature)
- `Option<T>` where `T` is poolable

**Poolable Arc types**:
- `Arc<T>` - Drop-in replacement for `std::sync::Arc` with pooling
- `TArc<T>` - Lighter-weight Arc using `triomphe::Arc` (with `triomphe` feature)

**Custom types**: Implement the `Poolable` trait (and optionally `IsoPoolable` for local pooling).

## Features

- **`triomphe`** (default): Enable `TArc<T>` poolable Arc
- **`indexmap`** (default): Enable pooling for `IndexMap` and `IndexSet`
- **`serde`** (default): Serialize/deserialize support for pooled types
