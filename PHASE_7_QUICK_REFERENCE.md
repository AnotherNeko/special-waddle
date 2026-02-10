# Phase 7: Quick Reference - Running Tests & Interpreting Results

## One-Command Testing

### Run Everything (All 44 tests)
```bash
cd rust && cargo test --release
```

Expected output (last line):
```
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured
```

---

## Focused Testing

### Truth Tests Only (Correctness Validation)
```bash
cd rust && cargo test --release -- --nocapture test_algorithm_comparison
```

Expected:
- `test_algorithm_comparison_truth_128cubed ... ok` (zero mismatches)
- `test_algorithm_comparison_conservation_128cubed ... ok` (exact sum match)

**What it tests**: New algorithm produces identical results to naive reference.

---

### Performance Benchmark Only
```bash
cd rust && cargo test --release -- --nocapture benchmark_naive_512x512x256_5steps
```

Expected output:
```
[BENCHMARK] Naive 512×512×256 (5 steps): ~3500 ms (~700 ms/step)
test automaton::field::tests::benchmark_naive_512x512x256_5steps ... ok
```

**Interpretation**:
- This is your **null hypothesis baseline** (~711 ms/step)
- New implementations must beat this
- Target: ≥ 2.0× speedup = ~355 ms/step

---

### Full Benchmark Suite (Scaling Analysis)
```bash
cd rust && cargo test --release -- --nocapture benchmark_suite_various_sizes
```

Expected output:
```
=== Performance Benchmark Suite ===

[16³] 0.00 ms/step
[64³] 1.80 ms/step
[128³] 21.20 ms/step
[256×256×128] 84.00 ms/step
[512×512×256] 694.00 ms/step

=== End Benchmark Suite ===
```

**What to look for**:
- Large jumps = cache misses (e.g., 64³ to 128³ crosses L3 boundary)
- Linear scaling after jump = predictable DRAM-bound behavior
- Compare new implementation against this suite

---

### Memory Analysis
```bash
cd rust && cargo test --release -- --nocapture measure_memory_footprint
```

Expected:
```
=== Memory Footprint ===

128³: 2097152 cells = 8.0 MB (cells: 8.0 MB, overhead: 40 B)
256×256×128: 8388608 cells = 32.0 MB (cells: 32.0 MB, overhead: 40 B)
512×512×256: 67108864 cells = 256.0 MB (cells: 256.0 MB, overhead: 40 B)
```

---

## Adding a New Optimized Implementation

### Step 1: Create the optimized function
```rust
/// Alternative implementation (e.g., tiled version)
pub fn field_step_tiled(field: &mut Field) {
    // Your optimized code here
    // Must produce identical results to field_step()
    // Must maintain conservation law
}
```

### Step 2: Add truth test
```rust
#[test]
fn test_algorithm_comparison_truth_128cubed_tiled() {
    let width = 128i16;
    let height = 128i16;
    let depth = 128i16;
    let diffusion_rate = 3u8;

    let reference_cells = generate_noisy_state(width, height, depth, 42);

    let mut naive_field = create_field(width, height, depth, diffusion_rate);
    let mut tiled_field = create_field(width, height, depth, diffusion_rate);

    naive_field.cells = reference_cells.clone();
    tiled_field.cells = reference_cells.clone();

    for _ in 0..4 {
        field_step(&mut naive_field);
        field_step_tiled(&mut tiled_field);  // <-- NEW
    }

    let mut mismatches = 0;
    for i in 0..naive_field.cells.len() {
        if naive_field.cells[i] != tiled_field.cells[i] {
            mismatches += 1;
            if mismatches <= 10 {
                eprintln!(
                    "Mismatch at index {}: naive={}, tiled={}",
                    i, naive_field.cells[i], tiled_field.cells[i]
                );
            }
        }
    }

    assert_eq!(mismatches, 0);
}
```

### Step 3: Add performance benchmark
```rust
#[test]
fn benchmark_tiled_512x512x256_5steps() {
    let width = 512i16;
    let height = 512i16;
    let depth = 256i16;
    let diffusion_rate = 3u8;

    let reference_cells = generate_noisy_state(width, height, depth, 9999);

    let mut field = create_field(width, height, depth, diffusion_rate);
    field.cells = reference_cells;

    let start = std::time::Instant::now();

    for _ in 0..5 {
        field_step_tiled(&mut field);  // <-- NEW
    }

    let elapsed = start.elapsed();

    eprintln!(
        "[BENCHMARK] Tiled 512×512×256 (5 steps): {} ms ({:.2} ms/step)",
        elapsed.as_millis(),
        elapsed.as_millis() as f64 / 5.0
    );

    assert!(elapsed.as_secs_f64() < 15.0);
}
```

### Step 4: Run and compare
```bash
cd rust && cargo test --release -- --nocapture test_algorithm_comparison_truth_128cubed_tiled
cd rust && cargo test --release -- --nocapture benchmark_tiled_512x512x256_5steps
```

### Step 5: Calculate speedup
```
Speedup = naive_time / optimized_time
         = 711 ms/step / <your_time> ms/step
```

**Acceptance**:
- ✓ Truth test passes (zero mismatches)
- ✓ Conservation passes (if you add it)
- ✓ Speedup ≥ 1.2× (target: 2.0-2.5×)

---

## Interpreting Test Failures

### Truth Test Mismatch
```
Mismatch at index 1234567: naive=50000, tiled=49999
Algorithm mismatch: 1234 cells differ
```

**Diagnosis**:
- Likely rounding/off-by-one error in your implementation
- Check boundary conditions (edges of grid)
- Verify all three axes are applied correctly
- Compare cell-by-cell against naive algorithm on small 8³ grid

### Conservation Test Failure
```
Conservation failed: 999999999 != 999999998
```

**Diagnosis**:
- Mass leaked or created somewhere
- Check: Are you cloning new_cells correctly?
- Verify apply order: do all three axes finish before next step?
- Ensure no `.max(0)` is truncating negative flows incorrectly

### Performance Regression
```
Severe performance regression: took 10.50s (expected <15s for 5 steps)
```

**Diagnosis**:
- System load (check `top` or `watch`)
- CPU throttling (check `/proc/cpuinfo`)
- Cache thrashing (profile with `perf`)
- Might still be faster than naive on high-load system

---

## Performance Interpretation Guide

### Baseline Performance
```
Naive 512×512×256: 711 ms/step
```

### What to Expect from Optimizations

**Tier 1 (Tiling)**: 1.8-2.5× → **285-395 ms/step**
- Cache efficiency dramatically improves
- Most realistic first optimization

**Tier 2 (Register Reuse)**: 1.3-1.5× → **475-547 ms/step**
- Incremental improvement on top of Tier 1
- Stacks with Tier 1 for ~2.5-3.5× combined

**Tier 3 (Thread Tuning)**: 1.1-1.2× → **593-647 ms/step**
- Minimal benefit unless memory-bound contention is severe
- Only worthwhile after Tier 1 frees up bandwidth

**Parallel (Tier 1 + Rayon)**: 2.5-3.5× on 10-core system
- Ideal: 711 / 10 = 71 ms/step (10× perfect scaling)
- Realistic: 200-285 ms/step (2.5-3.5× with cache coherency overhead)

---

## Reproducibility & Determinism

All benchmarks use **fixed seeds** so results are deterministic:
- `seed=42` for truth test (128³)
- `seed=2024` for conservation test
- `seed=9999` for production benchmark (512×512×256)
- `seed=100-104` for scaling suite

**Same code + same seed = same timing** (modulo ±5% system noise)

If you get suspicious results:
- Run 3 times, report average
- Check system load: `uptime`
- Check CPU scaling: `cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_driver`

---

## Debugging Tips

### Too Much Output?
```bash
cargo test --release -- --nocapture test_algorithm_comparison_truth_128cubed 2>&1 | grep -v "Mismatch"
```

### Want to Profile?
```bash
cargo build --release
perf record -g target/release/voxel_automata-d3685de81ea52633
perf report
```

### Check Memory Allocations?
```bash
cargo test --release --lib -- --nocapture measure_memory_footprint
```

### Run Just One Test?
```bash
cargo test --release benchmark_naive_512x512x256_5steps -- --nocapture --exact
```

---

## Commit Message Template

When you have a successful optimization:

```
Phase 7: Implement field_step_tiled() - Tier 1 optimization

- Implement spatial tiling (32³ cache-resident blocks)
- Process all 3 axes within each tile (loop fusion)
- Rayon parallelization over tiles

PERFORMANCE RESULTS (512×512×256, 5 steps):
- Naive baseline: 711 ms/step
- Tiled optimized: 295 ms/step
- Speedup: 2.41×

VALIDATION:
- Truth test (128³, 4 steps): PASS (zero mismatches)
- Conservation (128³): PASS (exact mass balance)
- Scaling suite: 16³ to 512×512×256 all PASS

All 44 tests passing.
```

---

## Quick Stats Table for Comparison

Keep this handy when optimizing:

| Implementation | Grid | Time/Step | Speedup | Notes |
|---|---|---|---|---|
| Naive (baseline) | 512×512×256 | 711 ms | 1.0× | Reference |
| **Tiled** | 512×512×256 | **~295 ms** | **2.41×** | TARGET |
| **Tiled + Reg Reuse** | 512×512×256 | **~200 ms** | **3.6×** | STRETCH |
| **Parallel Tiled** | 512×512×256 | **~100 ms** | **7.1×** | IDEAL |

---

**Last Updated**: Phase 7 Framework  
**Questions?** Check `PHASE_7_TESTING_FRAMEWORK.md` for detailed methodology
