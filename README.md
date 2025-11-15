# Thead Safe Object Pool

This is a general purpose thread safe object pool that supports most
data structures in the standard library, as well as many useful
external data structures such as IndexMap, triomphe::Arc, etc.

There are two kinds of pools, global, and local.

## Global Pools

Objects allocated from a global pool always return to the pool they
were allocated from, regardless of which thread drops the object. A
lock free queue is used, which imposes some overhead, but is necessary
in cases where one thread will primarially allocate the objects while
other threads consume them. If your objects will always stay on the
same thread, or, all threads will allocate and free them equally then
you should use a local pool.

In this example one task is always generating the message and another
task is always consuming the message. If we used a local pool, then
likely the batches freed on the consumer would just build up on the
thread that usually runs that task, and the producer would need to
keep allocating new batches because nothing would ever be returned to
it's pool.

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
// malloc again, and free is never called except before
// exit.

// Depending on the platform allocator this is usually faster that a
// constant churn of malloc/free ops. Whether or not it's faster on a
// particular platform, it is more determanistic across platforms. Yes
// the platform allocator may pull all the tricks in the book and
// might even perform better, but move to some other platform and
// performance is awful again.
```

## Local Pools

Local pools can be thought of as a more ergonomic way to create a
`thread_local!`. There is no need to wrap uses in a function like
`with_borrow_mut`, and you can in fact own the objects, pass them
between threads, etc. The primary difference between local pools and
global pools is that objects allocated from a local pool always return
to the pool associated with the thread that drops them. Other than
this, they are significantly faster, and more convenent to use, so
they should be the default in cases where the allocation pattern
doesn't require a global pool.

```rust
use poolshark::local::LPooled;
use std::{collections::HashSet, hash::Hash};

// dedup an unsorted vec. this will only allocate memory on,
// - the first call
// - if deduping a vec that is bigger than any previously seen
// - if deduping a vec that is bigger than the max length allowed in the pool
fn unsorted_dedup_stable<T: Hash + Eq>(v: &mut Vec<T>) {
    let mut set: LPooled<HashSet<&T>> = LPooled::take(); // take set from the pool or allocate it
    let mut remove: LPooled<Vec<usize>> = LPooled::take(); // take remove from the pool or allocate it
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
