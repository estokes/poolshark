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
