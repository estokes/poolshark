use super::global::{
    arc::{Arc, TArc},
    Pool, RawPool,
};
use crate::{local::LPooled, IsoPoolable};
use fxhash::FxHashMap;
use std::{
    collections::HashMap,
    hash::{BuildHasher, Hash},
};

/*
Sat Nov 15 02:08:42 PM EST 2025

==56172==
==56172== HEAP SUMMARY:
==56172==     in use at exit: 504 bytes in 2 blocks
==56172==   total heap usage: 11,052 allocs, 11,050 frees, 86,399,371 bytes allocated
==56172==
==56172== LEAK SUMMARY:
==56172==    definitely lost: 0 bytes in 0 blocks
==56172==    indirectly lost: 0 bytes in 0 blocks
==56172==      possibly lost: 48 bytes in 1 blocks
==56172==    still reachable: 456 bytes in 1 blocks
==56172==         suppressed: 0 bytes in 0 blocks
==56172== Rerun with --leak-check=full to see details of leaked memory
==56172==
==56172== For lists of detected and suppressed errors, rerun with: -s
==56172== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
*/
#[test]
fn normal_pool() {
    for _ in 0..100 {
        let pool: Pool<Vec<usize>> = Pool::new(1024, 1024);
        let mut v0 = pool.take();
        let mut v1 = pool.take();
        v0.reserve(100);
        v1.reserve(100);
        let (v0a, v1a) = (v0.as_ptr().addr(), v1.as_ptr().addr());
        let (v0c, v1c) = (v0.capacity(), v1.capacity());
        for _ in 0..100 {
            drop(v0);
            drop(v1);
            v0 = pool.take();
            v1 = pool.take();
            assert_eq!(v0.as_ptr().addr(), v0a);
            assert_eq!(v1.as_ptr().addr(), v1a);
            assert_eq!(v0.capacity(), v0c);
            assert_eq!(v1.capacity(), v1c);
            assert_eq!(v0.len(), 0);
            assert_eq!(v1.len(), 0);
            for i in 0..100 {
                v0.push(i);
                v1.push(i);
            }
            assert_eq!(pool.try_take(), None);
        }
        // vectors larger than 1024 will not be saved in the pool
        for _ in 0..100 {
            assert_eq!(pool.try_take(), None);
            let mut v2 = pool.take();
            assert_eq!(v2.capacity(), 0);
            v2.reserve(1025);
            for i in 0..1025 {
                v2.push(i);
            }
        }
        // add to pool
        drop(v0);
        // add to pool
        drop(v1);
        // should drop everything in the pool run under valgrind leak
        // check to ensure both v0 and v1 are actually freed
        drop(pool);
    }
}

/*
Sat Nov 15 02:08:42 PM EST 2025

==55566==
==55566== HEAP SUMMARY:
==55566==     in use at exit: 504 bytes in 2 blocks
==55566==   total heap usage: 663 allocs, 661 frees, 194,572 bytes allocated
==55566==
==55566== LEAK SUMMARY:
==55566==    definitely lost: 0 bytes in 0 blocks
==55566==    indirectly lost: 0 bytes in 0 blocks
==55566==      possibly lost: 48 bytes in 1 blocks
==55566==    still reachable: 456 bytes in 1 blocks
==55566==         suppressed: 0 bytes in 0 blocks
==55566== Rerun with --leak-check=full to see details of leaked memory
==55566==
==55566== For lists of detected and suppressed errors, rerun with: -s
==55566== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
*/
#[test]
fn local_pool() {
    let mut hmp0 = None;
    let mut hmp1 = None;
    let mut hmp2 = None;
    fn check_ptr<K: Hash + Eq, V, R: BuildHasher + Default>(
        orig: &mut Option<usize>,
        hm: &LPooled<HashMap<K, V, R>>,
    ) {
        let p = hm as *const LPooled<HashMap<K, V, R>> as usize;
        match orig {
            Some(orig) => assert_eq!(p, *orig),
            None => *orig = Some(p),
        }
    }
    let d0 = <FxHashMap<i32, i32> as IsoPoolable>::DISCRIMINANT;
    let d1 = <FxHashMap<usize, usize> as IsoPoolable>::DISCRIMINANT;
    let d2 = <HashMap<usize, usize> as IsoPoolable>::DISCRIMINANT;
    assert!(d0 != d1);
    assert!(d0 != d2);
    assert!(d1 != d2);
    for _ in 0..1000 {
        let mut hm0 = LPooled::<FxHashMap<i32, i32>>::take();
        let mut hm1 = LPooled::<FxHashMap<usize, usize>>::take();
        let mut hm2 = LPooled::<HashMap<usize, usize>>::take();
        check_ptr(&mut hmp0, &hm0);
        check_ptr(&mut hmp1, &hm1);
        check_ptr(&mut hmp2, &hm2);
        hm0.insert(42, 0);
        hm0.insert(0, 42);
        hm1.insert(0, 42);
        hm1.insert(42, 0);
        hm2.insert(0, 0);
        hm2.insert(1, 1);
    }
}

/*
Sat Nov 15 02:08:42 PM EST 2025

==55999==
==55999== HEAP SUMMARY:
==55999==     in use at exit: 504 bytes in 2 blocks
==55999==   total heap usage: 21,252 allocs, 21,250 frees, 1,989,957 bytes allocated
==55999==
==55999== LEAK SUMMARY:
==55999==    definitely lost: 0 bytes in 0 blocks
==55999==    indirectly lost: 0 bytes in 0 blocks
==55999==      possibly lost: 48 bytes in 1 blocks
==55999==    still reachable: 456 bytes in 1 blocks
==55999==         suppressed: 0 bytes in 0 blocks
==55999== Rerun with --leak-check=full to see details of leaked memory
==55999==
==55999== For lists of detected and suppressed errors, rerun with: -s
==55999== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
*/
#[test]
fn tarc_pool() {
    for _ in 0..100 {
        let pool: RawPool<TArc<String>> = RawPool::new(1024, 1);
        let mut v0 = TArc::new(&pool, "0".to_string());
        let mut v1 = TArc::new(&pool, "0".to_string());
        let v0a = v0.as_ptr().addr();
        let v1a = v1.as_ptr().addr();
        for i in 0..100 {
            drop(v0);
            drop(v1);
            v0 = TArc::new(&pool, i.to_string());
            v1 = TArc::new(&pool, i.to_string());
            assert_eq!(v0.as_ptr().addr(), v0a);
            assert_eq!(v1.as_ptr().addr(), v1a);
            assert_eq!(pool.try_take(), None);
            let v2 = v0.clone();
            let v3 = v1.clone();
            // drops v0 and v1, but they won't go back into the pool
            // because strong_count > 1.
            v0 = v2;
            v1 = v3;
            assert_eq!(pool.try_take(), None);
        }
        drop(v0);
        drop(v1);
        drop(pool)
    }
}

/*
Sat Nov 15 02:08:42 PM EST 2025

==56080==
==56080== HEAP SUMMARY:
==56080==     in use at exit: 504 bytes in 2 blocks
==56080==   total heap usage: 21,253 allocs, 21,251 frees, 1,991,551 bytes allocated
==56080==
==56080== LEAK SUMMARY:
==56080==    definitely lost: 0 bytes in 0 blocks
==56080==    indirectly lost: 0 bytes in 0 blocks
==56080==      possibly lost: 48 bytes in 1 blocks
==56080==    still reachable: 456 bytes in 1 blocks
==56080==         suppressed: 0 bytes in 0 blocks
==56080== Rerun with --leak-check=full to see details of leaked memory
==56080==
==56080== For lists of detected and suppressed errors, rerun with: -s
==56080== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
*/
#[test]
fn arc_pool() {
    for _ in 0..100 {
        let pool: RawPool<Arc<String>> = RawPool::new(1024, 1);
        let mut v0 = Arc::new(&pool, "0".to_string());
        let mut v1 = Arc::new(&pool, "0".to_string());
        let v0a = v0.as_ptr().addr();
        let v1a = v1.as_ptr().addr();
        for i in 0..100 {
            drop(v0);
            drop(v1);
            v0 = Arc::new(&pool, i.to_string());
            v1 = Arc::new(&pool, i.to_string());
            assert_eq!(v0.as_ptr().addr(), v0a);
            assert_eq!(v1.as_ptr().addr(), v1a);
            assert_eq!(pool.try_take(), None);
            let v2 = v0.clone();
            let v3 = v1.clone();
            // drops v0 and v1, but they won't go back into the pool
            // because strong_count > 1.
            v0 = v2;
            v1 = v3;
            assert_eq!(pool.try_take(), None);
        }
        drop(v0);
        drop(v1);
        drop(pool)
    }
}
