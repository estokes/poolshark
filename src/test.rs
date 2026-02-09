use super::global::{
    arc::{Arc, TArc},
    Pool, RawPool,
};
use crate::{local::LPooled, IsoPoolable};
use fxhash::{FxHashMap, FxHashSet};
use indexmap::{IndexMap, IndexSet};
use std::collections::{HashMap, HashSet, VecDeque};

////////// normal pool tests //////////

#[test]
fn normal_pool_string() {
    let mut vp0 = None;
    let mut vp1 = None;
    for _ in 0..100 {
        let pool: Pool<String> = Pool::new(1024, 1024);
        let mut v0 = pool.take();
        let mut v1 = pool.take();
        v0.reserve(100);
        v1.reserve(100);
        check_ptr(&mut vp0, &v0);
        check_ptr(&mut vp1, &v1);
        let (v0c, v1c) = (v0.capacity(), v1.capacity());
        for _ in 0..100 {
            drop(v0);
            drop(v1);
            v0 = pool.take();
            v1 = pool.take();
            check_ptr(&mut vp0, &v0);
            check_ptr(&mut vp1, &v1);
            assert_eq!(v0.capacity(), v0c);
            assert_eq!(v1.capacity(), v1c);
            assert_eq!(v0.len(), 0);
            assert_eq!(v1.len(), 0);
            for _ in 0..100 {
                v0.push('c');
                v1.push('c');
            }
            assert_eq!(pool.try_take(), None);
        }
        // vectors larger than 1024 will not be saved in the pool
        for _ in 0..100 {
            assert_eq!(pool.try_take(), None);
            let mut v2 = pool.take();
            assert_eq!(v2.capacity(), 0);
            v2.reserve(1025);
            for _ in 0..1025 {
                v2.push('c');
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

macro_rules! mk_normal_pool_veclike {
    ($vec:ident, $push:ident) => {{
        let mut vp0 = None;
        let mut vp1 = None;
        for _ in 0..100 {
            let pool: Pool<$vec<usize>> = Pool::new(1024, 1024);
            let mut v0 = pool.take();
            let mut v1 = pool.take();
            v0.reserve(100);
            v1.reserve(100);
            check_ptr(&mut vp0, &v0);
            check_ptr(&mut vp1, &v1);
            let (v0c, v1c) = (v0.capacity(), v1.capacity());
            for _ in 0..100 {
                drop(v0);
                drop(v1);
                v0 = pool.take();
                v1 = pool.take();
                check_ptr(&mut vp0, &v0);
                check_ptr(&mut vp1, &v1);
                assert_eq!(v0.capacity(), v0c);
                assert_eq!(v1.capacity(), v1c);
                assert_eq!(v0.len(), 0);
                assert_eq!(v1.len(), 0);
                for i in 0..100 {
                    v0.$push(i);
                    v1.$push(i);
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
                    v2.$push(i);
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
    }};
}

#[test]
fn normal_pool_vec() {
    mk_normal_pool_veclike!(Vec, push)
}

#[test]
fn normal_pool_vecdeque() {
    mk_normal_pool_veclike!(VecDeque, push_back)
}

fn check_ptr<T>(orig: &mut Option<usize>, hm: &T) {
    let p = hm as *const T as usize;
    match orig {
        Some(orig) => assert_eq!(p, *orig),
        None => *orig = Some(p),
    }
}

macro_rules! mk_normal_pool_hashmap {
    ($hash:ident) => {{
        let mut hmp0 = None;
        let mut hmp1 = None;
        for _ in 0..100 {
            let pool: Pool<$hash<usize, usize>> = Pool::new(1024, 1024);
            let mut v0 = pool.take();
            let mut v1 = pool.take();
            check_ptr(&mut hmp0, &*v0);
            check_ptr(&mut hmp1, &*v1);
            v0.reserve(100);
            v1.reserve(100);
            let (v0c, v1c) = (v0.capacity(), v1.capacity());
            for i in 0..100 {
                drop(v0);
                drop(v1);
                v0 = pool.take();
                v1 = pool.take();
                check_ptr(&mut hmp0, &*v0);
                check_ptr(&mut hmp1, &*v1);
                assert_eq!(v0.capacity(), v0c);
                assert_eq!(v1.capacity(), v1c);
                assert_eq!(v0.len(), 0);
                assert_eq!(v1.len(), 0);
                for j in 0..100 {
                    v0.insert(j, i);
                    v1.insert(j, i);
                }
                assert_eq!(pool.try_take(), None);
            }
            drop(v0);
            drop(v1);
            drop(pool);
        }
    }};
}

#[test]
fn normal_pool_hashmap() {
    mk_normal_pool_hashmap!(HashMap)
}

#[test]
fn normal_pool_fxhashmap() {
    mk_normal_pool_hashmap!(FxHashMap)
}

#[test]
fn normal_pool_indexmap() {
    mk_normal_pool_hashmap!(IndexMap)
}

macro_rules! mk_normal_pool_hashset {
    ($hash:ident) => {{
        let mut hmp0 = None;
        let mut hmp1 = None;
        for _ in 0..100 {
            let pool: Pool<$hash<usize>> = Pool::new(1024, 1024);
            let mut v0 = pool.take();
            let mut v1 = pool.take();
            check_ptr(&mut hmp0, &*v0);
            check_ptr(&mut hmp1, &*v1);
            v0.reserve(100);
            v1.reserve(100);
            let (v0c, v1c) = (v0.capacity(), v1.capacity());
            for i in 0..100 {
                drop(v0);
                drop(v1);
                v0 = pool.take();
                v1 = pool.take();
                check_ptr(&mut hmp0, &*v0);
                check_ptr(&mut hmp1, &*v1);
                assert_eq!(v0.capacity(), v0c);
                assert_eq!(v1.capacity(), v1c);
                assert_eq!(v0.len(), 0);
                assert_eq!(v1.len(), 0);
                for j in 0..100 {
                    v0.insert(j + i);
                    v1.insert(j + i);
                }
                assert_eq!(pool.try_take(), None);
            }
            drop(v0);
            drop(v1);
            drop(pool);
        }
    }};
}

#[test]
fn normal_pool_hashset() {
    mk_normal_pool_hashset!(HashSet)
}

#[test]
fn normal_pool_fxhashset() {
    mk_normal_pool_hashset!(FxHashSet)
}

#[test]
fn normal_pool_indexset() {
    mk_normal_pool_hashset!(IndexSet)
}

////////// local pool tests //////////

#[test]
fn local_pool_string() {
    let mut vp0 = None;
    let mut vp1 = None;
    for _ in 0..100 {
        let pool: Pool<String> = Pool::new(1024, 1024);
        let mut v0 = LPooled::<String>::take();
        let mut v1 = LPooled::<String>::take();
        v0.reserve(100);
        v1.reserve(100);
        check_ptr(&mut vp0, &v0);
        check_ptr(&mut vp1, &v1);
        let (v0c, v1c) = (v0.capacity(), v1.capacity());
        for _ in 0..100 {
            drop(v0);
            drop(v1);
            v0 = LPooled::take();
            v1 = LPooled::take();
            check_ptr(&mut vp0, &v0);
            check_ptr(&mut vp1, &v1);
            assert_eq!(v0.capacity(), v0c);
            assert_eq!(v1.capacity(), v1c);
            assert_eq!(v0.len(), 0);
            assert_eq!(v1.len(), 0);
            for _ in 0..100 {
                v0.push('c');
                v1.push('c');
            }
            assert_eq!(pool.try_take(), None);
        }
        // vectors larger than 1024 will not be saved in the pool
        for _ in 0..100 {
            assert_eq!(pool.try_take(), None);
            let mut v2 = pool.take();
            assert_eq!(v2.capacity(), 0);
            v2.reserve(1025);
            for _ in 0..1025 {
                v2.push('c');
            }
        }
        // add to pool
        drop(v0);
        // add to pool
        drop(v1);
    }
}

macro_rules! mk_local_pool_veclike {
    ($vec:ident, $alt:ident, $push:ident) => {{
        let mut vp0 = None;
        let mut vp1 = None;
        let mut vp2 = None;
        for _ in 0..100 {
            let mut v0 = LPooled::<$vec<i32>>::take();
            let mut v1 = LPooled::<$vec<usize>>::take();
            let v2 = LPooled::<$alt<usize>>::take();
            let d0 = <$vec<i32> as IsoPoolable>::DISCRIMINANT;
            let d1 = <$vec<usize> as IsoPoolable>::DISCRIMINANT;
            let d2 = <$alt<usize> as IsoPoolable>::DISCRIMINANT;
            assert!(d0 != d1);
            assert!(d0 != d2);
            assert!(d1 != d2);
            v0.reserve(100);
            v1.reserve(100);
            check_ptr(&mut vp0, &v0);
            check_ptr(&mut vp1, &v1);
            check_ptr(&mut vp2, &v2);
            let (v0c, v1c) = (v0.capacity(), v1.capacity());
            for _ in 0..100 {
                drop(v0);
                drop(v1);
                v0 = LPooled::<$vec<i32>>::take();
                v1 = LPooled::<$vec<usize>>::take();
                check_ptr(&mut vp0, &v0);
                check_ptr(&mut vp1, &v1);
                assert_eq!(v0.capacity(), v0c);
                assert_eq!(v1.capacity(), v1c);
                assert_eq!(v0.len(), 0);
                assert_eq!(v1.len(), 0);
                for i in 0..100 {
                    v0.$push(i);
                    v1.$push(i as usize);
                }
            }
        }
    }};
}

#[test]
fn local_pool_vec() {
    mk_local_pool_veclike!(Vec, VecDeque, push)
}

#[test]
fn local_pool_vecdeque() {
    mk_local_pool_veclike!(VecDeque, Vec, push_back)
}

macro_rules! mk_local_pool_hashmap {
    ($hash:ident, $alt:ident) => {{
        let mut hmp0 = None;
        let mut hmp1 = None;
        let mut hmp2 = None;
        let d0 = <$hash<i32, i32> as IsoPoolable>::DISCRIMINANT;
        let d1 = <$hash<usize, usize> as IsoPoolable>::DISCRIMINANT;
        let d2 = <$alt<usize, usize> as IsoPoolable>::DISCRIMINANT;
        assert!(d0 != d1);
        assert!(d0 != d2);
        assert!(d1 != d2);
        for _ in 0..1000 {
            let mut hm0 = LPooled::<$hash<i32, i32>>::take();
            let mut hm1 = LPooled::<$hash<usize, usize>>::take();
            let mut hm2 = LPooled::<$alt<usize, usize>>::take();
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
    }};
}

#[test]
fn local_pool_hashmap() {
    mk_local_pool_hashmap!(HashMap, FxHashMap)
}

#[test]
fn local_pool_fxhashmap() {
    mk_local_pool_hashmap!(FxHashMap, HashMap)
}

#[test]
fn local_pool_indexmap() {
    mk_local_pool_hashmap!(IndexMap, HashMap)
}

macro_rules! mk_local_pool_hashset {
    ($hash:ident, $alt:ident) => {{
        let mut hmp0 = None;
        let mut hmp1 = None;
        let mut hmp2 = None;
        let d0 = <$hash<i32> as IsoPoolable>::DISCRIMINANT;
        let d1 = <$hash<usize> as IsoPoolable>::DISCRIMINANT;
        let d2 = <$alt<usize> as IsoPoolable>::DISCRIMINANT;
        dbg!(d1);
        dbg!(d2);
        assert!(d0 != d1);
        assert!(d0 != d2);
        assert!(d1 != d2);
        for _ in 0..1000 {
            let mut hm0 = LPooled::<$hash<i32>>::take();
            let mut hm1 = LPooled::<$hash<usize>>::take();
            let mut hm2 = LPooled::<$alt<usize>>::take();
            check_ptr(&mut hmp0, &hm0);
            check_ptr(&mut hmp1, &hm1);
            check_ptr(&mut hmp2, &hm2);
            hm0.insert(42);
            hm0.insert(0);
            hm1.insert(0);
            hm1.insert(42);
            hm2.insert(0);
            hm2.insert(1);
        }
    }};
}

#[test]
fn local_pool_hashset() {
    mk_local_pool_hashset!(HashSet, IndexSet)
}

#[test]
fn local_pool_fxhashset() {
    mk_local_pool_hashset!(FxHashSet, HashSet)
}

#[test]
fn local_pool_indexset() {
    mk_local_pool_hashset!(IndexSet, FxHashSet)
}

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
