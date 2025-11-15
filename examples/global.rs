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
