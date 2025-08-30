use super::global::{
    Pool, RawPool,
    arc::{Arc, TArc},
};
use crate::local::LPooled;
use fxhash::FxHashMap;
use std::hash::Hash;

/*
==373258==
==373258== HEAP SUMMARY:
==373258==     in use at exit: 504 bytes in 2 blocks
==373258==   total heap usage: 11,053 allocs, 11,051 frees, 86,398,708 bytes allocated
==373258==
==373258== LEAK SUMMARY:
==373258==    definitely lost: 0 bytes in 0 blocks
==373258==    indirectly lost: 0 bytes in 0 blocks
==373258==      possibly lost: 48 bytes in 1 blocks
==373258==    still reachable: 456 bytes in 1 blocks
==373258==         suppressed: 0 bytes in 0 blocks
==373258== Rerun with --leak-check=full to see details of leaked memory
==373258==
==373258== For lists of detected and suppressed errors, rerun with: -s
==373258== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
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
==373122==
==373122== HEAP SUMMARY:
==373122==     in use at exit: 504 bytes in 2 blocks
==373122==   total heap usage: 661 allocs, 659 frees, 144,609 bytes allocated
==373122==
==373122== LEAK SUMMARY:
==373122==    definitely lost: 0 bytes in 0 blocks
==373122==    indirectly lost: 0 bytes in 0 blocks
==373122==      possibly lost: 48 bytes in 1 blocks
==373122==    still reachable: 456 bytes in 1 blocks
==373122==         suppressed: 0 bytes in 0 blocks
==373122== Rerun with --leak-check=full to see details of leaked memory
==373122==
==373122== For lists of detected and suppressed errors, rerun with: -s
==373122== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
*/
#[test]
fn local_pool() {
    let mut hmp0 = None;
    let mut hmp1 = None;
    fn check_ptr<K: Hash + Eq, V>(orig: &mut Option<usize>, hm: &LPooled<FxHashMap<K, V>>) {
        let p = hm as *const LPooled<FxHashMap<K, V>> as usize;
        match orig {
            Some(orig) => assert_eq!(p, *orig),
            None => *orig = Some(p),
        }
    }
    for _ in 0..100 {
        let mut hm0 = LPooled::<FxHashMap<i32, i32>>::take();
        let mut hm1 = LPooled::<FxHashMap<usize, usize>>::take();
        check_ptr(&mut hmp0, &hm0);
        check_ptr(&mut hmp1, &hm1);
        hm0.insert(42, 0);
        hm0.insert(0, 42);
        hm1.insert(0, 42);
        hm1.insert(42, 0);
    }
}

/*
==373438==
==373438== HEAP SUMMARY:
==373438==     in use at exit: 504 bytes in 2 blocks
==373438==   total heap usage: 21,253 allocs, 21,251 frees, 1,989,294 bytes allocated
==373438==
==373438== LEAK SUMMARY:
==373438==    definitely lost: 0 bytes in 0 blocks
==373438==    indirectly lost: 0 bytes in 0 blocks
==373438==      possibly lost: 48 bytes in 1 blocks
==373438==    still reachable: 456 bytes in 1 blocks
==373438==         suppressed: 0 bytes in 0 blocks
==373438== Rerun with --leak-check=full to see details of leaked memory
==373438==
==373438== For lists of detected and suppressed errors, rerun with: -s
==373438== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
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
==373504==
==373504== HEAP SUMMARY:
==373504==     in use at exit: 504 bytes in 2 blocks
==373504==   total heap usage: 41,878 allocs, 41,876 frees, 3,902,901 bytes allocated
==373504==
==373504== LEAK SUMMARY:
==373504==    definitely lost: 0 bytes in 0 blocks
==373504==    indirectly lost: 0 bytes in 0 blocks
==373504==      possibly lost: 48 bytes in 1 blocks
==373504==    still reachable: 456 bytes in 1 blocks
==373504==         suppressed: 0 bytes in 0 blocks
==373504== Rerun with --leak-check=full to see details of leaked memory
==373504==
==373504== For lists of detected and suppressed errors, rerun with: -s
==373504== ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)
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
