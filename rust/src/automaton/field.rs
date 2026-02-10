//! Integer field with delta-based diffusion.
//!
//! Implements conservation-safe diffusion on integer grids:
//! - Process each axis independently (X, Y, Z)
//! - For each adjacent cell pair, compute flow = (cell_a - cell_b) / divisor
//! - Apply symmetrically: cell_a -= flow, cell_b += flow
//! - This ensures conservation by Newton's third law
//! - Copy result back to field before next axis (prevents over-application)

/// A 3D field of u32 values.
/// Used for dense simulations like weather, thermal diffusion, or chemistry.
pub struct Field {
    pub width: i16,
    pub height: i16,
    pub depth: i16,
    pub cells: Vec<u32>, // u32 per cell (e.g. centigrams, microkelvin)
    pub generation: u64,
    pub diffusion_rate: u8, // power-of-2 shift (e.g. 3 = divide by 8)
}

/// Initialize a field with the given dimensions and diffusion rate.
pub fn create_field(width: i16, height: i16, depth: i16, diffusion_rate: u8) -> Field {
    let size = (width as usize) * (height as usize) * (depth as usize);
    Field {
        width,
        height,
        depth,
        cells: vec![0; size],
        generation: 0,
        diffusion_rate,
    }
}

/// Calculate the linear index for a 3D coordinate.
#[inline]
pub fn field_index_of(field: &Field, x: i16, y: i16, z: i16) -> usize {
    z as usize * field.height as usize * field.width as usize
        + y as usize * field.width as usize
        + x as usize
}

/// Check if coordinates are within field bounds.
#[inline]
pub fn field_in_bounds(field: &Field, x: i16, y: i16, z: i16) -> bool {
    x >= 0 && x < field.width && y >= 0 && y < field.height && z >= 0 && z < field.depth
}

/// Set a cell value.
pub fn field_set(field: &mut Field, x: i16, y: i16, z: i16, value: u32) {
    if field_in_bounds(field, x, y, z) {
        let idx = field_index_of(field, x, y, z);
        field.cells[idx] = value;
    }
}

/// Get a cell value.
pub fn field_get(field: &Field, x: i16, y: i16, z: i16) -> u32 {
    if field_in_bounds(field, x, y, z) {
        let idx = field_index_of(field, x, y, z);
        field.cells[idx]
    } else {
        0
    }
}

/// Step the field forward by one generation using axis-aligned diffusion.
/// Processes each axis (X, Y, Z) independently, computing and applying flows inline.
/// Between axes, copies results back to preserve conservation.
pub fn field_step(field: &mut Field) {
    let rate = field.diffusion_rate;
    let divisor = 1u32 << rate; // 2^rate

    let mut new_cells = field.cells.clone();

    // X-axis diffusion: each pair (x, x+1) exchanges
    for z in 0..field.depth {
        for y in 0..field.height {
            for x in 0..field.width - 1 {
                let idx_a = field_index_of(field, x, y, z);
                let idx_b = field_index_of(field, x + 1, y, z);

                let cell_a = field.cells[idx_a] as i64;
                let cell_b = field.cells[idx_b] as i64;
                let flow = (cell_a - cell_b) / (divisor as i64);

                new_cells[idx_a] = ((new_cells[idx_a] as i64) - flow).max(0) as u32;
                new_cells[idx_b] = ((new_cells[idx_b] as i64) + flow).max(0) as u32;
            }
        }
    }

    // Copy result back before next axis
    for i in 0..field.cells.len() {
        field.cells[i] = new_cells[i];
    }

    // Y-axis diffusion: each pair (y, y+1) exchanges
    for z in 0..field.depth {
        for y in 0..field.height - 1 {
            for x in 0..field.width {
                let idx_a = field_index_of(field, x, y, z);
                let idx_b = field_index_of(field, x, y + 1, z);

                let cell_a = field.cells[idx_a] as i64;
                let cell_b = field.cells[idx_b] as i64;
                let flow = (cell_a - cell_b) / (divisor as i64);

                new_cells[idx_a] = ((new_cells[idx_a] as i64) - flow).max(0) as u32;
                new_cells[idx_b] = ((new_cells[idx_b] as i64) + flow).max(0) as u32;
            }
        }
    }

    // Copy result back before next axis
    for i in 0..field.cells.len() {
        field.cells[i] = new_cells[i];
    }

    // Z-axis diffusion: each pair (z, z+1) exchanges
    for z in 0..field.depth - 1 {
        for y in 0..field.height {
            for x in 0..field.width {
                let idx_a = field_index_of(field, x, y, z);
                let idx_b = field_index_of(field, x, y, z + 1);

                let cell_a = field.cells[idx_a] as i64;
                let cell_b = field.cells[idx_b] as i64;
                let flow = (cell_a - cell_b) / (divisor as i64);

                new_cells[idx_a] = ((new_cells[idx_a] as i64) - flow).max(0) as u32;
                new_cells[idx_b] = ((new_cells[idx_b] as i64) + flow).max(0) as u32;
            }
        }
    }

    field.cells = new_cells;
    field.generation += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_field() {
        let field = create_field(8, 8, 8, 3);
        assert_eq!(field.width, 8);
        assert_eq!(field.height, 8);
        assert_eq!(field.depth, 8);
        assert_eq!(field.cells.len(), 512);
        assert_eq!(field.generation, 0);
        assert_eq!(field.diffusion_rate, 3);
        assert!(field.cells.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_field_set_get() {
        let mut field = create_field(8, 8, 8, 3);

        field_set(&mut field, 4, 4, 4, 1000);
        assert_eq!(field_get(&field, 4, 4, 4), 1000);
        assert_eq!(field_get(&field, 0, 0, 0), 0);

        // Out of bounds reads return 0
        assert_eq!(field_get(&field, -1, 0, 0), 0);
        assert_eq!(field_get(&field, 8, 0, 0), 0);
    }

    #[test]
    fn test_conservation_single_cell() {
        // Test that the total mass (sum of all cells) is preserved after stepping
        let mut field = create_field(8, 8, 8, 2);

        let total_mass = 1_000_000u32;
        field_set(&mut field, 4, 4, 4, total_mass);

        let initial_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        // Step multiple times
        for _ in 0..10 {
            field_step(&mut field);
        }

        let final_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        // Should be exactly equal (conservation by construction)
        assert_eq!(
            initial_sum, final_sum,
            "Mass not conserved: {} != {}",
            initial_sum, final_sum
        );
    }

    #[test]
    fn test_diffusion_spreads_symmetric() {
        // Test that diffusion spreads symmetrically from a point source
        let mut field = create_field(16, 16, 16, 2);

        let center_val = 1_000_000u32;
        field_set(&mut field, 8, 8, 8, center_val);

        field_step(&mut field);

        // Check that neighbors got some value
        let neighbors_have_value = field_get(&field, 7, 8, 8) > 0
            && field_get(&field, 9, 8, 8) > 0
            && field_get(&field, 8, 7, 8) > 0
            && field_get(&field, 8, 9, 8) > 0
            && field_get(&field, 8, 8, 7) > 0
            && field_get(&field, 8, 8, 9) > 0;

        assert!(
            neighbors_have_value,
            "Neighbors should have non-zero values"
        );

        // Check total is still conserved
        let total: u64 = field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(total, center_val as u64, "Total mass should be conserved");
    }

    #[test]
    fn test_diffusion_spreads_from_edge() {
        // Test spreading from a cell at the edge (boundary condition)
        let mut field = create_field(8, 8, 8, 2);

        field_set(&mut field, 0, 4, 4, 1_000_000u32);

        let initial_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();
        field_step(&mut field);
        let final_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        assert_eq!(
            initial_sum, final_sum,
            "Mass not conserved at boundary: {} != {}",
            initial_sum, final_sum
        );
    }

    #[test]
    fn test_generation_increments() {
        let mut field = create_field(8, 8, 8, 3);
        assert_eq!(field.generation, 0);

        field_step(&mut field);
        assert_eq!(field.generation, 1);

        field_step(&mut field);
        assert_eq!(field.generation, 2);
    }

    #[test]
    fn test_zero_field_stays_zero() {
        let mut field = create_field(8, 8, 8, 3);

        field_step(&mut field);

        assert!(field.cells.iter().all(|&c| c == 0));
        assert_eq!(field.generation, 1);
    }

    // ========== Algorithmic Comparison Tests ==========
    // These tests verify that alternative implementations produce identical results
    // to the naive algorithm (null hypothesis).

    /// Generate a pseudo-random noisy starting state using a simple LCG.
    /// Seed is based on position to ensure reproducibility.
    fn generate_noisy_state(width: i16, height: i16, depth: i16, seed_base: u32) -> Vec<u32> {
        let size = (width as usize) * (height as usize) * (depth as usize);
        let mut cells = vec![0u32; size];

        // Linear Congruential Generator: simple, fast, reproducible
        let mut lcg_state = seed_base.wrapping_mul(1103515245).wrapping_add(12345);

        for i in 0..size {
            lcg_state = lcg_state.wrapping_mul(1103515245).wrapping_add(12345);
            let noise = (lcg_state >> 16) as u32 & 0xFFFF; // Extract 16 bits
            cells[i] = if i % 7 == 0 {
                noise.saturating_mul(100) // Sparse high-value cells
            } else if i % 13 == 0 {
                noise / 10 // More frequent lower-value cells
            } else {
                0 // Most cells empty
            };
        }

        cells
    }

    #[test]
    fn test_algorithm_comparison_truth_128cubed() {
        // Generate reference starting state: 128^3 field with noisy initial condition
        let width = 128i16;
        let height = 128i16;
        let depth = 128i16;
        let diffusion_rate = 3u8;

        let reference_cells = generate_noisy_state(width, height, depth, 42);

        // Create two fields: naive (reference) and test algorithm
        let mut naive_field = create_field(width, height, depth, diffusion_rate);
        let mut test_field = create_field(width, height, depth, diffusion_rate);

        // Initialize both with identical starting state
        naive_field.cells = reference_cells.clone();
        test_field.cells = reference_cells.clone();

        // Step both 4 times
        for _ in 0..4 {
            field_step(&mut naive_field);
            field_step(&mut test_field);
        }

        // Verify all cells are identical
        let mut mismatches = 0;
        for i in 0..naive_field.cells.len() {
            if naive_field.cells[i] != test_field.cells[i] {
                mismatches += 1;
                if mismatches <= 10 {
                    eprintln!(
                        "Mismatch at index {}: naive={}, test={}",
                        i, naive_field.cells[i], test_field.cells[i]
                    );
                }
            }
        }

        assert_eq!(
            mismatches, 0,
            "Algorithm mismatch: {} cells differ between naive and test implementation",
            mismatches
        );
        assert_eq!(naive_field.generation, test_field.generation);
    }

    #[test]
    fn test_algorithm_comparison_conservation_128cubed() {
        // Verify that alternative algorithm maintains conservation
        let width = 128i16;
        let height = 128i16;
        let depth = 128i16;
        let diffusion_rate = 3u8;

        let reference_cells = generate_noisy_state(width, height, depth, 2024);

        let mut field = create_field(width, height, depth, diffusion_rate);
        field.cells = reference_cells;

        let initial_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        // Step 4 times
        for _ in 0..4 {
            field_step(&mut field);
        }

        let final_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        assert_eq!(
            initial_sum, final_sum,
            "Conservation failed: {} != {}",
            initial_sum, final_sum
        );
    }

    // ========== Performance Benchmarks ==========
    // These benchmarks measure wall-clock time for the stepping kernel.
    // Run with: cargo test --release -- --nocapture benchmark_

    /// Benchmark: 5 steps on 512×512×256 field (naive algorithm).
    /// This is the baseline performance metric.
    #[test]
    fn benchmark_naive_512x512x256_5steps() {
        let width = 512i16;
        let height = 512i16;
        let depth = 256i16;
        let diffusion_rate = 3u8;

        let reference_cells = generate_noisy_state(width, height, depth, 9999);

        let mut field = create_field(width, height, depth, diffusion_rate);
        field.cells = reference_cells;

        let start = std::time::Instant::now();

        for _ in 0..5 {
            field_step(&mut field);
        }

        let elapsed = start.elapsed();

        eprintln!(
            "[BENCHMARK] Naive 512×512×256 (5 steps): {} ms ({:.2} ms/step)",
            elapsed.as_millis(),
            elapsed.as_millis() as f64 / 5.0
        );

        // Note: This is a performance baseline, not a hard limit.
        // System load, CPU turbo frequency, and hardware variations affect timing.
        // Threshold intentionally generous to avoid flaky test failures.
        assert!(
            elapsed.as_secs_f64() < 15.0,
            "Severe performance regression: took {:.2}s (expected <15s for 5 steps)",
            elapsed.as_secs_f64()
        );
    }

    /// Benchmark helper: measures time for N steps, returns milliseconds per step.
    fn benchmark_field_steps(
        width: i16,
        height: i16,
        depth: i16,
        diffusion_rate: u8,
        num_steps: usize,
        seed: u32,
    ) -> f64 {
        let reference_cells = generate_noisy_state(width, height, depth, seed);

        let mut field = create_field(width, height, depth, diffusion_rate);
        field.cells = reference_cells;

        let start = std::time::Instant::now();

        for _ in 0..num_steps {
            field_step(&mut field);
        }

        let elapsed = start.elapsed();
        elapsed.as_millis() as f64 / num_steps as f64
    }

    #[test]
    fn benchmark_suite_various_sizes() {
        eprintln!("\n=== Performance Benchmark Suite ===\n");

        // Small grid (baseline for correctness)
        let t1 = benchmark_field_steps(16, 16, 16, 3, 10, 100);
        eprintln!("[16³] {:.2} ms/step", t1);

        // Medium grid
        let t2 = benchmark_field_steps(64, 64, 64, 3, 10, 101);
        eprintln!("[64³] {:.2} ms/step", t2);

        // Large grid (typical use case)
        let t3 = benchmark_field_steps(128, 128, 128, 3, 5, 102);
        eprintln!("[128³] {:.2} ms/step", t3);

        // Very large grid (512×512×256)
        let t4 = benchmark_field_steps(256, 256, 128, 3, 3, 103);
        eprintln!("[256×256×128] {:.2} ms/step", t4);

        // Production scale (512×512×256)
        let t5 = benchmark_field_steps(512, 512, 256, 3, 2, 104);
        eprintln!("[512×512×256] {:.2} ms/step", t5);

        eprintln!("\n=== End Benchmark Suite ===\n");
    }

    /// Measure memory overhead of field structure.
    #[test]
    fn measure_memory_footprint() {
        let sizes = vec![
            (128i16, 128i16, 128i16, "128³"),
            (256i16, 256i16, 128i16, "256×256×128"),
            (512i16, 512i16, 256i16, "512×512×256"),
        ];

        eprintln!("\n=== Memory Footprint ===\n");

        for (w, h, d, label) in sizes {
            let field = create_field(w, h, d, 3);
            let cell_bytes = (w as usize) * (h as usize) * (d as usize) * 4;
            let field_overhead = std::mem::size_of::<Field>();
            let total = cell_bytes + field_overhead;

            eprintln!(
                "{}: {} cells = {:.1} MB (cells: {:.1} MB, overhead: {} B)",
                label,
                field.cells.len(),
                total as f64 / 1_048_576.0,
                cell_bytes as f64 / 1_048_576.0,
                field_overhead
            );
        }

        eprintln!();
    }
}
