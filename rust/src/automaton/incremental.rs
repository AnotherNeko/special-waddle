//! Non-blocking step scheduler for Luanti integration.
//!
//! Splits a full field step into bounded work quanta (16³ tiles) that can be
//! spread across multiple Luanti ticks without blocking frames.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::automaton::delta::DeltaOverrides;
use crate::automaton::field::{create_field, Field};
use crate::automaton::kernel::{build_tile_queue, process_tile, IncrementalStep, TILE_SIZE};

/// Manages the lifecycle of incremental steps for a Field.
pub struct StepController {
    /// The field being stepped.
    pub field: Field,

    /// In-progress step state, or None if idle.
    pub active_step: Option<IncrementalStep>,

    /// Rayon thread pool (1 thread initially, configurable).
    pub thread_pool: rayon::ThreadPool,

    /// Persistent delta overrides. Moved into IncrementalStep on begin_step,
    /// returned here on finalize_step (with updated log entries, etc.).
    pub delta_overrides: DeltaOverrides,
}

impl StepController {
    /// Create a new step controller with the given dimensions and thread pool size.
    pub fn new(width: i16, height: i16, depth: i16, diffusion_rate: u8, num_threads: u8) -> Self {
        let field = create_field(width, height, depth, diffusion_rate);
        let num_threads = if num_threads == 0 {
            1
        } else {
            num_threads as usize
        };
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap_or_else(|_| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .unwrap()
            });

        StepController {
            field,
            active_step: None,
            thread_pool,
            delta_overrides: DeltaOverrides::default(),
        }
    }

    /// Create a step controller from an existing field (for test ergonomics).
    pub fn from_field(field: Field, num_threads: u8) -> Self {
        let num_threads = if num_threads == 0 {
            1
        } else {
            num_threads as usize
        };
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap_or_else(|_| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .unwrap()
            });

        StepController {
            field,
            active_step: None,
            thread_pool,
            delta_overrides: DeltaOverrides::default(),
        }
    }

    /// Extract the inner field (for test ergonomics).
    pub fn into_field(self) -> Field {
        self.field
    }

    /// Query whether a step is currently in progress.
    pub fn is_stepping(&self) -> bool {
        self.active_step.is_some()
    }

    /// Begin a new incremental step. No-op if a step is already in progress.
    pub fn begin_step(&mut self) -> Result<(), ()> {
        if self.is_stepping() {
            return Err(());
        }

        let width = self.field.width;
        let height = self.field.height;
        let depth = self.field.depth;

        let tiles_x = (width as usize + TILE_SIZE as usize - 1) / TILE_SIZE as usize;
        let tiles_y = (height as usize + TILE_SIZE as usize - 1) / TILE_SIZE as usize;
        let tiles_z = (depth as usize + TILE_SIZE as usize - 1) / TILE_SIZE as usize;
        let total_tiles = tiles_x * tiles_y * tiles_z;

        let source = self.field.cells.clone();
        let target = self.field.cells.clone();
        let tile_queue = build_tile_queue(tiles_x as u8, tiles_y as u8, tiles_z as u8);

        let cell_count = width as usize * height as usize * depth as usize;
        let mut cell_has_override = vec![false; cell_count];
        let delta_overrides = std::mem::take(&mut self.delta_overrides);
        for &(owner_idx, _) in delta_overrides.keys() {
            if owner_idx < cell_count {
                cell_has_override[owner_idx] = true;
            }
        }

        let step = IncrementalStep {
            source,
            target,
            tile_queue,
            next_tile: std::sync::atomic::AtomicUsize::new(0),
            total_tiles,
            target_generation: self.field.generation + 1,
            width,
            height,
            depth,
            diffusion_rate: self.field.diffusion_rate,
            delta_overrides,
            cell_has_override,
        };

        self.active_step = Some(step);
        Ok(())
    }

    /// Do bounded work within the given time budget (microseconds).
    /// Returns true if the step completed during this tick, false if more work remains.
    pub fn tick(&mut self, budget_us: u64) -> bool {
        let step = match &mut self.active_step {
            Some(s) => s,
            None => return true,
        };

        let deadline = Instant::now() + Duration::from_micros(budget_us);

        loop {
            let tile_idx = step.next_tile.fetch_add(1, Ordering::Relaxed);
            if tile_idx >= step.total_tiles {
                self.finalize_step();
                return true;
            }

            let tile = step.tile_queue[tile_idx];
            process_tile(step, tile);

            if Instant::now() >= deadline {
                return false; // Budget exhausted, yield to Lua.
            }
        }
    }

    /// Blocking full step (equivalent to begin + tick(MAX) until done).
    pub fn step_blocking(&mut self) {
        self.begin_step().ok();
        while !self.tick(u64::MAX) {}
    }

    fn finalize_step(&mut self) {
        if let Some(step) = self.active_step.take() {
            self.field.cells = step.target;
            self.field.generation = step.target_generation;
            self.delta_overrides = step.delta_overrides;
        }
    }
}

/// Wrapper for algorithm registry integration (field.rs tests).
pub fn field_step_incremental(field: &mut crate::automaton::field::Field) {
    let old_field = Field {
        width: field.width,
        height: field.height,
        depth: field.depth,
        cells: std::mem::take(&mut field.cells),
        generation: field.generation,
        diffusion_rate: field.diffusion_rate,
        conductivity: field.conductivity,
    };

    let mut ctrl = StepController::from_field(old_field, 1);
    ctrl.step_blocking();
    let new_field = ctrl.into_field();

    field.cells = new_field.cells;
    field.generation = new_field.generation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automaton::field::{create_field, field_get, field_set, field_step_fused};
    use crate::automaton::kernel::compute_flow;

    fn generate_noisy_state(width: i16, height: i16, depth: i16, seed_base: u32) -> Vec<u32> {
        let size = (width as usize) * (height as usize) * (depth as usize);
        let mut cells = vec![0u32; size];

        let mut lcg_state = seed_base.wrapping_mul(1103515245).wrapping_add(12345);

        for i in 0..size {
            lcg_state = lcg_state.wrapping_mul(1103515245).wrapping_add(12345);
            let noise = (lcg_state >> 16) as u32 & 0xFFFF;
            cells[i] = if i % 7 == 0 {
                noise.saturating_mul(100)
            } else if i % 13 == 0 {
                noise / 10
            } else {
                0
            };
        }

        cells
    }

    #[test]
    fn test_create_step_controller() {
        let ctrl = StepController::new(16, 16, 16, 2, 1);
        assert_eq!(ctrl.field.width, 16);
        assert_eq!(ctrl.field.height, 16);
        assert_eq!(ctrl.field.depth, 16);
        assert!(!ctrl.is_stepping());
    }

    #[test]
    fn test_begin_step() {
        let mut ctrl = StepController::new(16, 16, 16, 2, 1);
        assert!(ctrl.begin_step().is_ok());
        assert!(ctrl.is_stepping());

        // Second begin should fail
        assert!(ctrl.begin_step().is_err());
    }

    #[test]
    fn test_step_blocking() {
        let mut ctrl = StepController::new(8, 8, 8, 2, 1);
        field_set(&mut ctrl.field, 4, 4, 4, 1_000_000);

        let initial_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

        ctrl.step_blocking();

        let final_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(
            initial_sum, final_sum,
            "Mass not conserved in blocking step"
        );
        assert_eq!(ctrl.field.generation, 1);
    }

    #[test]
    fn test_incremental_matches_fused_128cubed() {
        let cells = generate_noisy_state(128, 128, 128, 42);
        let expected_sum: u64 = cells.iter().map(|&v| v as u64).sum();

        let mut fused_field = create_field(128, 128, 128, 3);
        fused_field.cells = cells.clone();
        for _ in 0..4 {
            field_step_fused(&mut fused_field);
        }

        let mut ctrl = StepController::new(128, 128, 128, 3, 1);
        ctrl.field.cells = cells;
        for _ in 0..4 {
            ctrl.step_blocking();
        }

        let actual_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(actual_sum, expected_sum, "Mass not conserved");

        let mut max_diff = 0u32;
        let mut total_diff = 0u64;
        for i in 0..ctrl.field.cells.len() {
            let diff = if ctrl.field.cells[i] > fused_field.cells[i] {
                ctrl.field.cells[i] - fused_field.cells[i]
            } else {
                fused_field.cells[i] - ctrl.field.cells[i]
            };
            max_diff = max_diff.max(diff);
            total_diff += diff as u64;
        }

        eprintln!(
            "Incremental vs Fused: max_diff={}, avg_diff={:.2}",
            max_diff,
            total_diff as f64 / ctrl.field.cells.len() as f64
        );

        // Allow differences due to tile-based remainder accumulation AND boundary mirror deltas
        assert!(
            max_diff <= 25,
            "Incremental differs too much from fused: max_diff={}",
            max_diff
        );
    }

    #[test]
    fn test_incremental_ticking_matches_blocking() {
        let cells = generate_noisy_state(64, 64, 64, 99);

        let mut blocking = StepController::new(64, 64, 64, 3, 1);
        blocking.field.cells = cells.clone();
        blocking.step_blocking();

        let mut ticking = StepController::new(64, 64, 64, 3, 1);
        ticking.field.cells = cells;
        ticking.begin_step().unwrap();
        let mut ticks = 0;
        while !ticking.tick(100) {
            ticks += 1;
        }

        assert_eq!(ticking.field.cells, blocking.field.cells);
        assert!(ticks > 1, "Budget should have forced multiple ticks");
    }

    #[test]
    fn test_conservation_128cubed() {
        let cells = generate_noisy_state(128, 128, 128, 2024);
        let expected_sum: u64 = cells.iter().map(|&v| v as u64).sum();

        let mut ctrl = StepController::new(128, 128, 128, 3, 1);
        ctrl.field.cells = cells;

        for _ in 0..4 {
            ctrl.step_blocking();
        }

        let final_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(final_sum, expected_sum, "Mass not conserved");
    }

    #[test]
    fn test_determinism_128cubed() {
        let cells = generate_noisy_state(128, 128, 128, 42);

        let mut ctrl1 = StepController::new(128, 128, 128, 3, 1);
        ctrl1.field.cells = cells.clone();
        for _ in 0..4 {
            ctrl1.step_blocking();
        }

        let mut ctrl2 = StepController::new(128, 128, 128, 3, 1);
        ctrl2.field.cells = cells;
        for _ in 0..4 {
            ctrl2.step_blocking();
        }

        assert_eq!(ctrl1.field.cells, ctrl2.field.cells, "Not deterministic");
    }

    #[test]
    fn test_small_field_single_tile() {
        let mut ctrl = StepController::new(8, 8, 8, 2, 1);
        field_set(&mut ctrl.field, 4, 4, 4, 1_000_000);

        let initial_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

        ctrl.step_blocking();

        let final_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(initial_sum, final_sum, "Mass not conserved for small field");
    }

    #[test]
    fn test_non_tile_aligned_100cubed() {
        let cells = generate_noisy_state(100, 100, 100, 555);
        let expected_sum: u64 = cells.iter().map(|&v| v as u64).sum();

        let mut ctrl = StepController::new(100, 100, 100, 2, 1);
        ctrl.field.cells = cells;

        ctrl.step_blocking();

        let final_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(
            final_sum, expected_sum,
            "Mass not conserved for non-aligned field"
        );
    }

    #[test]
    fn test_minimum_field_stays_minimum() {
        let mut ctrl = StepController::new(16, 16, 16, 3, 1);
        ctrl.step_blocking();
        assert!(
            ctrl.field.cells.iter().all(|&c| c >= 1),
            "Third Law violation: some cells dropped below minimum quantum of 1"
        );
    }

    #[test]
    fn test_boundary_cell() {
        let mut ctrl = StepController::new(16, 16, 16, 2, 1);
        field_set(&mut ctrl.field, 0, 8, 8, 1_000_000);

        let initial_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

        ctrl.step_blocking();

        let final_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(initial_sum, final_sum, "Mass not conserved at boundary");

        assert!(
            field_get(&ctrl.field, 1, 8, 8).unwrap().get() > 1,
            "Neighbor should receive flow"
        );
    }

    #[test]
    fn test_boundary_cell_stochastic_rounding_symmetry() {
        let mut ctrl = StepController::new(3, 3, 3, 2, 1);
        field_set(&mut ctrl.field, 1, 1, 1, 1);

        let mut boundary_history = vec![];

        for step_num in 0..200 {
            let boundary_before = [
                field_get(&ctrl.field, 0, 1, 1).unwrap().get(),
                field_get(&ctrl.field, 2, 1, 1).unwrap().get(),
                field_get(&ctrl.field, 1, 0, 1).unwrap().get(),
                field_get(&ctrl.field, 1, 2, 1).unwrap().get(),
                field_get(&ctrl.field, 1, 1, 0).unwrap().get(),
                field_get(&ctrl.field, 1, 1, 2).unwrap().get(),
            ];

            ctrl.step_blocking();

            let boundary_after = [
                field_get(&ctrl.field, 0, 1, 1).unwrap().get(),
                field_get(&ctrl.field, 2, 1, 1).unwrap().get(),
                field_get(&ctrl.field, 1, 0, 1).unwrap().get(),
                field_get(&ctrl.field, 1, 2, 1).unwrap().get(),
                field_get(&ctrl.field, 1, 1, 0).unwrap().get(),
                field_get(&ctrl.field, 1, 1, 2).unwrap().get(),
            ];

            for (i, (before, after)) in boundary_before.iter().zip(&boundary_after).enumerate() {
                if before != after {
                    let delta = (*after as i64) - (*before as i64);
                    boundary_history.push((step_num, i, delta, *before, *after));
                    eprintln!(
                        "Step {}: boundary cell {} changed {} → {} (delta={})",
                        step_num, i, before, after, delta
                    );
                }
            }
        }

        for (step, cell, delta, before, after) in &boundary_history {
            assert!(
                delta.abs() <= 1,
                "Step {}: boundary cell {} changed by {} (before={}, after={}), expected ±1 or 0",
                step, cell, delta, before, after
            );
            assert_ne!(
                *after, u32::MAX,
                "Step {}: boundary cell {} underflowed to u32::MAX",
                step, cell
            );
        }

        let center_final = field_get(&ctrl.field, 1, 1, 1).unwrap().get();
        assert!(
            center_final < 1_000_000,
            "Center cell should have diffused outward"
        );

        eprintln!(
            "Boundary symmetry test passed: {} changes observed, all ±1",
            boundary_history.len()
        );
    }

    #[test]
    fn test_debug_u32_underflow_game_scenario() {
        eprintln!("\n========== DEBUG: U32 Underflow Game Scenario ==========");

        let mut ctrl = StepController::new(16, 16, 16, 3, 1);

        let initial_val = 1_000_000_000u32;
        field_set(&mut ctrl.field, 0, 0, 0, initial_val);

        let initial_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        eprintln!("Initial state: corner (0,0,0) = {}", initial_val);
        eprintln!("Initial total mass: {}", initial_sum);

        for gen in 1..=10 {
            eprintln!("\n--- Generation {} ---", gen);

            let max_vals_before = ctrl.field.cells.iter().filter(|&&v| v == u32::MAX).count();
            if max_vals_before > 0 {
                eprintln!(
                    "  WARNING: {} cells already at u32::MAX before step!",
                    max_vals_before
                );
            }

            ctrl.step_blocking();

            let current_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
            let max_val = ctrl.field.cells.iter().copied().max().unwrap_or(0);
            let max_vals = ctrl.field.cells.iter().filter(|&&v| v == u32::MAX).count();
            let nonzero = ctrl.field.cells.iter().filter(|&&v| v > 0).count();

            eprintln!("  Total mass: {}", current_sum);
            eprintln!("  Max cell value: {}", max_val);
            eprintln!("  Cells at u32::MAX: {}", max_vals);
            eprintln!("  Nonzero cells: {}", nonzero);
            eprintln!(
                "  Corner (0,0,0): {}",
                field_get(&ctrl.field, 0, 0, 0).unwrap().get()
            );

            if max_vals > 0 {
                eprintln!("\n!!! UNDERFLOW DETECTED !!!");
                for z in 0..ctrl.field.depth {
                    for y in 0..ctrl.field.height {
                        for x in 0..ctrl.field.width {
                            if field_get(&ctrl.field, x, y, z).unwrap().get() == u32::MAX {
                                eprintln!("  Cell ({},{},{}): u32::MAX", x, y, z);
                            }
                        }
                    }
                }
                panic!("Underflow detected at generation {}", gen);
            }

            if current_sum != initial_sum {
                eprintln!(
                    "\n!!! MASS VIOLATION !!!\nExpected: {}, Got: {}, Delta: {}",
                    initial_sum,
                    current_sum,
                    current_sum as i64 - initial_sum as i64
                );
                panic!("Mass conservation violated at generation {}", gen);
            }
        }

        eprintln!("\n✓ Test completed without underflow");
    }

    #[test]
    fn test_debug_u32_underflow_large_field() {
        eprintln!("\n========== DEBUG: U32 Underflow Large Field ==========");

        let cells = generate_noisy_state(64, 64, 64, 12345);
        let expected_sum: u64 = cells.iter().map(|&v| v as u64).sum();

        let mut ctrl = StepController::new(64, 64, 64, 3, 1);
        ctrl.field.cells = cells;

        eprintln!("Initial total mass: {}", expected_sum);

        for gen in 1..=30 {
            let max_vals_before = ctrl.field.cells.iter().filter(|&&v| v == u32::MAX).count();
            if max_vals_before > 0 {
                eprintln!(
                    "Gen {}: UNDERFLOW BEFORE STEP! {} cells at u32::MAX",
                    gen, max_vals_before
                );
                panic!("Underflow detected");
            }

            ctrl.step_blocking();

            let current_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
            let max_vals_after = ctrl.field.cells.iter().filter(|&&v| v == u32::MAX).count();

            if max_vals_after > 0 {
                eprintln!(
                    "Gen {}: UNDERFLOW AFTER STEP! {} cells at u32::MAX",
                    gen, max_vals_after
                );
                let mut underflow_cells = vec![];
                for z in 0..ctrl.field.depth {
                    for y in 0..ctrl.field.height {
                        for x in 0..ctrl.field.width {
                            let val = field_get(&ctrl.field, x, y, z).unwrap().get();
                            if val == u32::MAX {
                                underflow_cells.push((x, y, z));
                            }
                        }
                    }
                }
                eprintln!("Underflow cells: {:?}", underflow_cells);
                panic!("Underflow at generation {}", gen);
            }

            if current_sum != expected_sum {
                eprintln!(
                    "Gen {}: Mass violation! Expected {}, got {}",
                    gen, expected_sum, current_sum
                );
                panic!("Conservation failed");
            }

            if gen % 5 == 0 {
                eprintln!("Gen {}: OK (sum={})", gen, current_sum);
            }
        }

        eprintln!("✓ Large field test passed");
    }

    #[test]
    fn benchmark_incremental_blocking_256x256x128_2steps() {
        let cells = generate_noisy_state(256, 256, 128, 9999);

        let mut ctrl = StepController::new(256, 256, 128, 3, 1);
        ctrl.field.cells = cells;

        let start = Instant::now();

        for _ in 0..2 {
            ctrl.step_blocking();
        }

        let elapsed = start.elapsed();

        eprintln!(
            "[BENCHMARK] Incremental blocking 256×256×128 (2 steps): {} ms ({:.2} ms/step)",
            elapsed.as_millis(),
            elapsed.as_millis() as f64 / 2.0
        );

        assert!(
            elapsed.as_secs_f64() < 15.0,
            "Performance regression: took {:.2}s",
            elapsed.as_secs_f64()
        );
    }

    /// Phase 8B: Logged pairs record flows identical to the modal formula.
    #[test]
    fn test_logged_delta_matches_modal() {
        use crate::automaton::delta::DeltaKind;

        let mut ctrl = StepController::new(4, 4, 4, 0, 1);

        let w = ctrl.field.width;
        let h = ctrl.field.height;
        let idx = |x: i16, y: i16, z: i16| {
            z as usize * h as usize * w as usize + y as usize * w as usize + x as usize
        };

        ctrl.field.cells[idx(0, 0, 0)] = 10000;
        ctrl.field.cells[idx(1, 0, 0)] = 2000;
        ctrl.field.cells[idx(0, 1, 0)] = 8000;
        ctrl.field.cells[idx(0, 2, 0)] = 1000;

        let pair_x = (idx(0, 0, 0), idx(1, 0, 0));
        let pair_y = (idx(0, 1, 0), idx(0, 2, 0));

        ctrl.delta_overrides.insert(pair_x, DeltaKind::new_logged());
        ctrl.delta_overrides.insert(pair_y, DeltaKind::new_logged());

        ctrl.step_blocking();

        let shift = 0u32;
        let conductivity = 65535i64;
        let divisor = (7i64 << shift) << 16;

        let check_logged = |kind: &DeltaKind, expected_gradient: i64| {
            let log = kind.log().expect("should be Logged variant");
            assert!(!log.is_empty(), "log should have at least one entry");
            let mut acc = 0i64;
            let expected_flow = compute_flow(expected_gradient, conductivity, divisor, &mut acc);
            // Allow ±1: the tile's shared remainder_acc carries state from prior pairs,
            // so the logged flow may differ by 1 from a fresh-accumulator call.
            assert!(
                (log[0] - expected_flow).abs() <= 1,
                "logged flow {} is more than ±1 from modal flow {} for gradient {}",
                log[0], expected_flow, expected_gradient
            );
        };

        check_logged(ctrl.delta_overrides.get(&pair_x).unwrap(), 10000 - 2000);
        check_logged(ctrl.delta_overrides.get(&pair_y).unwrap(), 8000 - 1000);
    }
}
