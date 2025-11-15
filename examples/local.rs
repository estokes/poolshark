use poolshark::local::LPooled;
use std::{collections::HashSet, hash::Hash};

// dedup an unsorted vec. this will only allocate memory on,
// - the first call
// - deduping a vec that is bigger than any previously seen
// - deduping a vec that is bigger than the max length allowed in the pool
fn unsorted_dedup_stable<T: Hash + Eq>(v: &mut Vec<T>) {
    let mut set: LPooled<HashSet<&T>> = LPooled::take(); // take set from the pool or allocate it
    let mut retain: LPooled<Vec<bool>> = LPooled::take(); // take retain from the pool or allocate it
    for t in v.iter() {
        retain.push(set.insert(t))
    }
    drop(set); // set is cleared and pushed to the thread local pool
    let mut i = 0;
    v.retain(|_| {
        let res = retain[i];
        i += 1;
        res
    })
    // retain is cleared and pushed to the thread local pool
}

fn main() {
    let mut v = vec!["one", "two", "one", "five", "three sir", "three", "four", "five"];
    println!("with dupes: {:?}", v);
    unsorted_dedup_stable(&mut v);
    println!("deduped: {:?}", v)
}
