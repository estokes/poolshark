use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};
use poolshark::global::Pool;
use poolshark::local::LPooled;
use std::collections::HashMap;
use std::sync::LazyLock;

// Global pool for cross-thread benchmarks
static GLOBAL_HASHMAP_POOL: LazyLock<Pool<HashMap<u64, u64>>> =
    LazyLock::new(|| Pool::new(1024, 1024));

static GLOBAL_VEC_POOL: LazyLock<Pool<Vec<u64>>> =
    LazyLock::new(|| Pool::new(1024, 1024));

static GLOBAL_STRING_POOL: LazyLock<Pool<String>> =
    LazyLock::new(|| Pool::new(1024, 1024));

const SIZES: [u64; 12] = [1, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];

// Benchmark: Vec operations with local pooling vs standard allocation
fn bench_vec(c: &mut Criterion) {
    let mut group = c.benchmark_group("vec");
    for size in SIZES.iter() {
        group.throughput(Throughput::Elements(*size as u64));

        // Standard allocation
        group.bench_with_input(BenchmarkId::new("standard", size), size, |b, &size| {
            b.iter(|| {
                let mut v = Vec::new();
                for i in 0..size {
                    v.push(black_box(i));
                }
                black_box(v);
            });
        });

        // Local pooling
        group.bench_with_input(
            BenchmarkId::new("local_pooled", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let mut v: LPooled<Vec<u64>> = LPooled::take();
                    for i in 0..size {
                        v.push(black_box(i));
                    }
                    black_box(&v);
                });
            },
        );

        // Global pooling
        group.bench_with_input(
            BenchmarkId::new("global_pooled", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let mut v = GLOBAL_VEC_POOL.take();
                    for i in 0..size {
                        v.push(black_box(i));
                    }
                    black_box(&v);
                });
            },
        );
    }

    group.finish();
}

// Benchmark: HashMap operations with local pooling vs standard allocation
fn bench_hashmap(c: &mut Criterion) {
    let mut group = c.benchmark_group("hashmap");

    for size in SIZES.iter() {
        group.throughput(Throughput::Elements(*size as u64));

        // Standard allocation
        group.bench_with_input(BenchmarkId::new("standard", size), size, |b, &size| {
            b.iter(|| {
                let mut map = HashMap::new();
                for i in 0..size {
                    map.insert(black_box(i), black_box(i * 2));
                }
                black_box(map);
            });
        });

        // Local pooling
        group.bench_with_input(
            BenchmarkId::new("local_pooled", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let mut map: LPooled<HashMap<u64, u64>> = LPooled::take();
                    for i in 0..size {
                        map.insert(black_box(i), black_box(i * 2));
                    }
                    black_box(&map);
                });
            },
        );

        // Local pooling
        group.bench_with_input(
            BenchmarkId::new("global_pooled", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let mut map = GLOBAL_HASHMAP_POOL.take();
                    for i in 0..size {
                        map.insert(black_box(i), black_box(i * 2));
                    }
                    black_box(&map);
                });
            },
        );
    }

    group.finish();
}

// Benchmark: String operations with local pooling vs standard allocation
fn bench_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("string");

    for size in SIZES.iter() {
        group.throughput(Throughput::Elements(*size as u64));

        // Standard allocation
        group.bench_with_input(BenchmarkId::new("standard", size), size, |b, &size| {
            b.iter(|| {
                let mut s = String::new();
                for _ in 0..size {
                    s.push_str(black_box("x"));
                }
                black_box(s);
            });
        });

        // Local pooling
        group.bench_with_input(
            BenchmarkId::new("local_pooled", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let mut s: LPooled<String> = LPooled::take();
                    for _ in 0..size {
                        s.push_str(black_box("x"));
                    }
                    black_box(&s);
                });
            },
        );

        // Global pooling
        group.bench_with_input(
            BenchmarkId::new("global_pooled", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let mut s = GLOBAL_STRING_POOL.take();
                    for _ in 0..size {
                        s.push_str(black_box("x"));
                    }
                    black_box(&s);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_vec, bench_hashmap, bench_string);
criterion_main!(benches);
