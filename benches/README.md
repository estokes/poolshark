# Poolshark Benchmarks

This directory contains benchmarks comparing poolshark's pooling performance against standard allocation.

## Running Benchmarks

Run all benchmarks:
```bash
cargo bench
```

Run specific benchmark:
```bash
cargo bench --bench pooling -- vec_operations
```

Generate HTML reports (saved to `target/criterion/`):
```bash
cargo bench
```

## Benchmark Categories

### 1. **vec_operations**
Compares `Vec` performance with local pooling vs standard allocation for different sizes (10, 100, 1000 elements).

### 2. **hashmap_operations**
Compares `HashMap` performance with local pooling vs standard allocation for different sizes.

### 3. **string_operations**
Compares `String` performance with local pooling vs standard allocation.

### 4. **repeated_allocations**
Stress test: 1000 allocate-use-drop cycles to measure the impact of pooling on allocation-heavy workloads.

### 5. **global_pooling**
Compares global pooling performance vs standard allocation (relevant for cross-thread scenarios).

### 6. **pool_overhead**
Measures the overhead of taking and returning objects from pools vs standard allocation.

## Expected Results

Pooling typically shows performance benefits when:
- **Repeated allocations**: The more times you allocate and drop containers, the more you benefit
- **Larger containers**: Containers with pre-allocated capacity benefit from reuse
- **Allocation-heavy workloads**: When allocation is a bottleneck

The overhead of pooling is minimal in the take/drop cycle, and you'll see significant improvements in repeated allocation scenarios.

## Notes

- Benchmarks use `black_box()` to prevent compiler optimizations from eliminating work
- Local pools are warmed up after the first few iterations
- Results will vary based on system allocator and hardware
