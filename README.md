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

```rust:examples/global.rs

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

```rust:examples/local.rs
