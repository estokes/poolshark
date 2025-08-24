# Thead Safe Object Pool

This is a general purpose thread safe object pool that supports most
data structures in the standard library, as well as many useful
external data structures such as IndexMap, triomphe::Arc, etc.

It limits the pool size as well as the element size to prevent memory
waste.

```rust
use std::sync::LazyLock;
use poolshark::Pool;

static POOL: LazyLock<Pool<Vec<String>>> = LazyLock::new(|| Pool::new(1024, 1024))

fn main() {
    let mut v = POOL.take();
    // do stuff with v
    drop(v) // v actually goes back into the pool
}
```
