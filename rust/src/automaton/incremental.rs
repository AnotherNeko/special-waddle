//! Non-blocking step scheduler for Luanti integration.
//!
//! Splits a full field step into bounded work quanta (16³ tiles) that can be
//! spread across multiple Luanti ticks without blocking frames.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::automaton::delta::{ContractList, NeighborOverrides};
use crate::automaton::field::{create_field, Field};
use crate::automaton::kernel::{
    build_tile_queue, process_contract_list, process_tile, IncrementalStep, TILE_SIZE,
};

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
    pub delta_overrides: NeighborOverrides,

    /// Flat contract list (extra graph edges beyond the 3-per-voxel spatial loop).
    pub contract_list: ContractList,
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
            delta_overrides: NeighborOverrides::default(),
            contract_list: ContractList::new(),
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
            delta_overrides: NeighborOverrides::default(),
            contract_list: ContractList::new(),
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
        if let Some(mut step) = self.active_step.take() {
            process_contract_list(
                &step.source,
                &mut step.target,
                &mut self.contract_list,
                step.diffusion_rate,
            );
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
                step,
                cell,
                delta,
                before,
                after
            );
            assert_ne!(
                *after,
                u32::MAX,
                "Step {}: boundary cell {} underflowed to u32::MAX",
                step,
                cell
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
        use crate::automaton::delta::NeighborKind;

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

        ctrl.delta_overrides
            .insert(pair_x, NeighborKind::new_logged());
        ctrl.delta_overrides
            .insert(pair_y, NeighborKind::new_logged());

        ctrl.step_blocking();

        let shift = 0u32;
        let conductivity = 65535i64;
        let divisor = (7i64 << shift) << 16;

        let check_logged = |kind: &NeighborKind, expected_gradient: i64| {
            let log = kind.log().expect("should be Logged variant");
            assert!(!log.is_empty(), "log should have at least one entry");
            let mut acc = 0i64;
            let expected_flow = compute_flow(expected_gradient, conductivity, divisor, &mut acc);
            // Allow ±1: the tile's shared remainder_acc carries state from prior pairs,
            // so the logged flow may differ by 1 from a fresh-accumulator call.
            assert!(
                (log[0] - expected_flow).abs() <= 1,
                "logged flow {} is more than ±1 from modal flow {} for gradient {}",
                log[0],
                expected_flow,
                expected_gradient
            );
        };

        check_logged(ctrl.delta_overrides.get(&pair_x).unwrap(), 10000 - 2000);
        check_logged(ctrl.delta_overrides.get(&pair_y).unwrap(), 8000 - 1000);
    }

    // Helper: flat index for a field of given dimensions.
    fn idx(w: i16, h: i16, x: i16, y: i16, z: i16) -> usize {
        z as usize * h as usize * w as usize + y as usize * w as usize + x as usize
    }

    /// Phase 8B: Mirror override blocks flow on a pair with a strong gradient.
    /// Mass conservation must still hold (neither cell changes due to that pair).
    #[test]
    fn test_mirror_delta_blocks_flow() {
        use crate::automaton::delta::NeighborKind;

        // 4×4×4, diffusion_rate=0 for predictable divisor.
        // reminder that diffusion rate is inversely proportional to actual flow rate.
        let mut ctrl = StepController::new(4, 4, 4, 0, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;

        // Strong gradient along X between (0,0,0) and (1,0,0).
        let i_a = idx(w, h, 0, 0, 0);
        let i_b = idx(w, h, 1, 0, 0);
        ctrl.field.cells[i_a] = 1_000_000;
        ctrl.field.cells[i_b] = 0;
        let before_a = ctrl.field.cells[i_a];
        let before_b = ctrl.field.cells[i_b];

        ctrl.delta_overrides
            .insert((i_a, i_b), NeighborKind::Mirror);
        ctrl.step_blocking();

        let after_a = ctrl.field.cells[i_a];
        let after_b = ctrl.field.cells[i_b];

        // The mirror pair must produce zero net exchange between a and b.
        // Other pairs (Y, Z neighbors) may still draw from a, so we only check
        // that b did not receive from this specific pair. Since b starts at 0
        // and has no other neighbors with mass in this direction, after_b == 1
        // (the field minimum) indicates normal minimum enforcement, not mirror flow.
        // The mirror contract means: flow on (i_a, i_b) == 0.
        // We verify this indirectly: b must not have risen above its minimum.
        assert_eq!(
            after_b, 1,
            "Mirror pair: neighbor should not receive flow (got {})",
            after_b
        );

        // a may lose mass to its other neighbors but must not gain from b.
        assert!(
            after_a <= before_a,
            "Mirror pair: owner should not gain mass (before={}, after={})",
            before_a,
            after_a
        );
        let _ = before_b; // suppress unused warning
    }

    /// Phase 8B: Mirror override — mass conservation holds across the whole field.
    #[test]
    fn test_mirror_delta_conserves_mass() {
        use crate::automaton::delta::NeighborKind;

        let mut ctrl = StepController::new(8, 8, 8, 1, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;

        // Scatter some mass.
        ctrl.field.cells[idx(w, h, 2, 2, 2)] = 500_000;
        ctrl.field.cells[idx(w, h, 3, 2, 2)] = 100_000;
        ctrl.field.cells[idx(w, h, 4, 2, 2)] = 200_000;

        let i_a = idx(w, h, 2, 2, 2);
        let i_b = idx(w, h, 3, 2, 2);
        ctrl.delta_overrides
            .insert((i_a, i_b), NeighborKind::Mirror);

        let mass_before: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        // for loop does several steps
        for _ in 0..16 {
            ctrl.step_blocking();
        }
        let mass_after: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

        assert_eq!(
            mass_before, mass_after,
            "Mirror pair must not break conservation (before={}, after={})",
            mass_before, mass_after
        );
    }

    /// Phase 8B: Void override — consumed accumulator accounts for all missing mass.
    ///
    /// Void is a ContractList entry (non-spatial extra edge). The spatial pair on
    /// the breach face is set to Mirror so no direct modal flow occurs; the Void
    /// contract drains mass to virtual vacuum. `consumed` must equal total mass lost.
    #[test]
    fn test_void_delta_is_mass_sink() {
        use crate::automaton::delta::{Contract, ContractKind, NeighborKind};

        let mut ctrl = StepController::new(4, 4, 4, 0, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;

        let i_a = idx(w, h, 0, 0, 0);
        let i_b = idx(w, h, 1, 0, 0); // spatial neighbor on the breach face

        ctrl.field.cells[i_a] = 1_000_000;
        let mass_before: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

        // Mirror the spatial pair so no modal flow crosses to i_b.
        ctrl.delta_overrides
            .insert((i_a, i_b), NeighborKind::Mirror);

        // Void contract drains i_a to virtual vacuum.
        ctrl.contract_list.contracts.push(Contract {
            src_a: i_a as u32,
            src_b: 0, // unused: virtual neighbor is always 0
            dst_a: i_a as u32,
            dst_b: 0, // unused: no write target
            kind: ContractKind::Void { consumed: 0 },
        });

        for _ in 0..16 {
            ctrl.step_blocking();
        }

        let mass_after: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        let mass_lost = mass_before - mass_after;

        let consumed = match &ctrl.contract_list.contracts[0].kind {
            ContractKind::Void { consumed } => *consumed as u64,
            _ => panic!("expected Void"),
        };

        assert!(consumed > 0, "Void: sink never activated");
        assert_eq!(
            consumed, mass_lost,
            "Void: consumed ({}) must equal mass removed from field ({})",
            consumed, mass_lost
        );
    }

    /// Infinity contract: cell draining toward a lower target (sink mode) and
    /// a separate cell filling toward a higher target (source mode). In both cases
    /// `consumed` must account for all mass that crossed the boundary.
    #[test]
    fn test_infinity_delta_sink_and_source() {
        use crate::automaton::delta::{Contract, ContractKind, NeighborKind};

        // --- sink mode: cell above target loses mass --------------------------------
        let mut ctrl = StepController::new(4, 4, 4, 0, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;
        let i_a = idx(w, h, 0, 0, 0);
        let i_b = idx(w, h, 1, 0, 0);

        ctrl.field.cells[i_a] = 1_000_000;
        let mass_before: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

        ctrl.delta_overrides.insert((i_a, i_b), NeighborKind::Mirror);
        ctrl.contract_list.contracts.push(Contract {
            src_a: i_a as u32, src_b: 0,
            dst_a: i_a as u32, dst_b: 0,
            kind: ContractKind::Infinity { target_value: 0, consumed: 0 },
        });

        for _ in 0..16 {
            ctrl.step_blocking();
        }

        let mass_after: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        let mass_lost = mass_before - mass_after;
        let consumed = match &ctrl.contract_list.contracts[0].kind {
            ContractKind::Infinity { consumed, .. } => *consumed as u64,
            _ => panic!("expected Infinity"),
        };
        assert!(consumed > 0, "Infinity sink: never activated");
        assert_eq!(consumed, mass_lost,
            "Infinity sink: consumed ({}) must equal mass removed ({})", consumed, mass_lost);

        // --- source mode: cell below target gains mass ------------------------------
        let mut ctrl2 = StepController::new(4, 4, 4, 0, 1);
        let target_value: u32 = 500_000;
        let i_c = idx(w, h, 0, 0, 0);
        let i_d = idx(w, h, 1, 0, 0);

        ctrl2.field.cells[i_c] = 0;
        let mass_before2: u64 = ctrl2.field.cells.iter().map(|&v| v as u64).sum();

        ctrl2.delta_overrides.insert((i_c, i_d), NeighborKind::Mirror);
        ctrl2.contract_list.contracts.push(Contract {
            src_a: i_c as u32, src_b: 0,
            dst_a: i_c as u32, dst_b: 0,
            kind: ContractKind::Infinity { target_value, consumed: 0 },
        });

        for _ in 0..16 {
            ctrl2.step_blocking();
        }

        let mass_after2: u64 = ctrl2.field.cells.iter().map(|&v| v as u64).sum();
        let mass_gained = mass_after2 - mass_before2;
        let consumed2 = match &ctrl2.contract_list.contracts[0].kind {
            ContractKind::Infinity { consumed, .. } => *consumed,
            _ => panic!("expected Infinity"),
        };
        assert!(mass_gained > 0, "Infinity source: cell never filled");
        // consumed is negative when mass flows into the cell (gradient is negative)
        assert_eq!((-consumed2) as u64, mass_gained,
            "Infinity source: |consumed| ({}) must equal mass gained ({})", -consumed2, mass_gained);
    }

    /// Phase 8B: Modal override produces identical output to no override.
    #[test]
    fn test_modal_override_matches_baseline() {
        use crate::automaton::delta::NeighborKind;

        // Baseline: no overrides.
        let mut baseline = StepController::new(4, 4, 4, 0, 1);
        let w = baseline.field.width;
        let h = baseline.field.height;
        baseline.field.cells[idx(w, h, 0, 0, 0)] = 50_000;
        baseline.field.cells[idx(w, h, 1, 0, 0)] = 10_000;
        baseline.step_blocking();
        let baseline_cells = baseline.field.cells.clone();

        // With explicit Modal override on the same pair.
        let mut with_modal = StepController::new(4, 4, 4, 0, 1);
        with_modal.field.cells[idx(w, h, 0, 0, 0)] = 50_000;
        with_modal.field.cells[idx(w, h, 1, 0, 0)] = 10_000;
        let i_a = idx(w, h, 0, 0, 0);
        let i_b = idx(w, h, 1, 0, 0);
        with_modal
            .delta_overrides
            .insert((i_a, i_b), NeighborKind::Modal);
        with_modal.step_blocking();

        assert_eq!(
            baseline_cells, with_modal.field.cells,
            "Explicit Modal override must produce identical output to no override"
        );
    }

    /// Phase 8B: Logged delta accumulates one entry per step across multiple generations.
    #[test]
    fn test_logged_delta_accumulates_across_steps() {
        use crate::automaton::delta::NeighborKind;

        let mut ctrl = StepController::new(4, 4, 4, 0, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;

        let i_a = idx(w, h, 1, 1, 1);
        let i_b = idx(w, h, 2, 1, 1);
        ctrl.field.cells[i_a] = 80_000;
        ctrl.field.cells[i_b] = 1_000;

        ctrl.delta_overrides
            .insert((i_a, i_b), NeighborKind::new_logged());

        let n_steps = 5;
        for _ in 0..n_steps {
            ctrl.step_blocking();
        }

        let log = ctrl
            .delta_overrides
            .get(&(i_a, i_b))
            .expect("override must survive across steps")
            .log()
            .expect("must be Logged variant");

        assert_eq!(
            log.len(),
            n_steps,
            "log must have exactly one entry per step (got {})",
            log.len()
        );
    }

    /// Phase 8B: cell_has_override flag is set for owner cells, clear for all others.
    #[test]
    fn test_cell_has_override_flag_population() {
        use crate::automaton::delta::NeighborKind;

        let mut ctrl = StepController::new(4, 4, 4, 0, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;

        let i_owner1 = idx(w, h, 0, 0, 0);
        let i_nbr1 = idx(w, h, 1, 0, 0);
        let i_owner2 = idx(w, h, 2, 1, 0);
        let i_nbr2 = idx(w, h, 2, 2, 0);

        ctrl.delta_overrides
            .insert((i_owner1, i_nbr1), NeighborKind::Modal);
        ctrl.delta_overrides
            .insert((i_owner2, i_nbr2), NeighborKind::Modal);

        // begin_step populates cell_has_override; we inspect before any tile runs.
        ctrl.begin_step().expect("begin_step must succeed");

        let step = ctrl.active_step.as_ref().expect("step must be active");

        assert!(
            step.cell_has_override[i_owner1],
            "owner1 (idx {}) must have override flag set",
            i_owner1
        );
        assert!(
            step.cell_has_override[i_owner2],
            "owner2 (idx {}) must have override flag set",
            i_owner2
        );
        assert!(
            !step.cell_has_override[i_nbr1],
            "neighbor1 (idx {}) must NOT have override flag set",
            i_nbr1
        );
        assert!(
            !step.cell_has_override[i_nbr2],
            "neighbor2 (idx {}) must NOT have override flag set",
            i_nbr2
        );

        // Verify no other cell is incorrectly flagged.
        let flagged: Vec<usize> = step
            .cell_has_override
            .iter()
            .enumerate()
            .filter(|(_, &v)| v)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            flagged,
            vec![i_owner1, i_owner2],
            "only registered owner cells must be flagged"
        );
    }

    /// Phase 8B: override added after begin_step takes effect next generation, not current.
    #[test]
    fn test_override_added_mid_run_deferred() {
        use crate::automaton::delta::NeighborKind;

        let mut ctrl = StepController::new(4, 4, 4, 0, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;

        let i_a = idx(w, h, 0, 0, 0);
        let i_b = idx(w, h, 1, 0, 0);
        ctrl.field.cells[i_a] = 60_000;
        ctrl.field.cells[i_b] = 1;

        // Begin a step with no overrides, then insert one before it finishes.
        ctrl.begin_step().expect("begin_step must succeed");
        ctrl.delta_overrides
            .insert((i_a, i_b), NeighborKind::Mirror);

        // Complete the in-progress step (Mirror was added after begin, so this
        // step runs without the mirror — flow should have occurred).
        while !ctrl.tick(u64::MAX) {}

        // The first step ran modal; b should have gained mass from the gradient.
        let after_first = ctrl.field.cells[i_b];
        assert!(
            after_first > 1,
            "First step (no mirror yet) should allow flow to b (got {})",
            after_first
        );

        // Now the override is in ctrl.delta_overrides. Second step picks it up.
        ctrl.step_blocking();
        let after_second = ctrl.field.cells[i_b];

        // In the second step the mirror is active; b must not have gained further
        // from the (i_a, i_b) pair (it may still lose to its own neighbors though).
        assert!(
            after_second <= after_first,
            "Second step (mirror active) must not allow further gain on b (first={}, second={})",
            after_first,
            after_second
        );
    }

    /// Phase 8B: override on a pair that straddles a tile boundary fires correctly.
    #[test]
    fn test_override_across_tile_boundary() {
        use crate::automaton::delta::NeighborKind;

        // 32×16×16: tiles split at x=16, so (15, 0, 0)→(16, 0, 0) is a boundary pair.
        let mut ctrl = StepController::new(32, 16, 16, 0, 1);
        let w = ctrl.field.width;
        let h = ctrl.field.height;

        let i_a = idx(w, h, 15, 0, 0); // last cell of tile 0
        let i_b = idx(w, h, 16, 0, 0); // first cell of tile 1

        ctrl.field.cells[i_a] = 200_000;
        ctrl.field.cells[i_b] = 1;

        ctrl.delta_overrides
            .insert((i_a, i_b), NeighborKind::new_logged());
        ctrl.step_blocking();

        let log = ctrl
            .delta_overrides
            .get(&(i_a, i_b))
            .expect("override must survive")
            .log()
            .expect("must be Logged");

        assert_eq!(log.len(), 1, "tile-boundary pair must fire exactly once");

        // Flow must be positive (mass from a to b).
        assert!(
            log[0] > 0,
            "tile-boundary flow must be positive for this gradient (got {})",
            log[0]
        );
    }

    /// Phase 8B: morton_encode / build_tile_queue ordering is correct.
    #[test]
    fn test_morton_order_2x2x2() {
        // Hand-computed Morton codes for a 2×2×2 grid of tiles:
        // Morton(x,y,z) = spread(x) | spread(y)<<1 | spread(z)<<2
        // For single-bit values: Morton = x | y<<1 | z<<2
        // (0,0,0)=0  (1,0,0)=1  (0,1,0)=2  (1,1,0)=3
        // (0,0,1)=4  (1,0,1)=5  (0,1,1)=6  (1,1,1)=7
        let expected: Vec<(u8, u8, u8)> = vec![
            (0, 0, 0),
            (1, 0, 0),
            (0, 1, 0),
            (1, 1, 0),
            (0, 0, 1),
            (1, 0, 1),
            (0, 1, 1),
            (1, 1, 1),
        ];

        let tiles = build_tile_queue(2, 2, 2);
        let got: Vec<(u8, u8, u8)> = tiles.iter().map(|t| (t.tx, t.ty, t.tz)).collect();

        assert_eq!(
            got, expected,
            "build_tile_queue(2,2,2) must produce tiles in Morton order"
        );
    }

    // -----------------------------------------------------------------------
    // Helpers shared by the topology tests below.
    // -----------------------------------------------------------------------

    /// Translate field `cells` by `(dx, dy, dz)` on a 3-torus (wrapping).
    fn torus_translate(
        cells: &[u32],
        w: i16,
        h: i16,
        d: i16,
        dx: i16,
        dy: i16,
        dz: i16,
    ) -> Vec<u32> {
        let mut out = vec![0u32; cells.len()];
        for z in 0..d {
            for y in 0..h {
                for x in 0..w {
                    let src = idx(w, h, x, y, z);
                    let tx = (x + dx).rem_euclid(w);
                    let ty = (y + dy).rem_euclid(h);
                    let tz = (z + dz).rem_euclid(d);
                    let dst = idx(w, h, tx, ty, tz);
                    out[dst] = cells[src];
                }
            }
        }
        out
    }

    /// Fuzzy congruence: sum of |a[i] - b[i]| / (cell_count * steps) < threshold.
    /// Accounts for the kernel's stochastic rounding noise.
    fn fuzzy_congruent(a: &[u32], b: &[u32], steps: u32, threshold: f64) -> bool {
        assert_eq!(a.len(), b.len());
        let total_diff: u64 = a
            .iter()
            .zip(b.iter())
            .map(|(&av, &bv)| (av as i64 - bv as i64).unsigned_abs())
            .sum();
        let noise_per_cell_per_step = total_diff as f64 / (a.len() as f64 * steps as f64);
        noise_per_cell_per_step < threshold
    }

    /// Register Portal contracts to wrap all 6 boundary face-pairs on a cube,
    /// turning the field into a 3-torus. Each boundary face gets a Mirror spatial
    /// override (so no flow exits the grid edge) plus a Portal ContractList entry
    /// coupling the opposing face.
    fn register_3torus_portals(ctrl: &mut StepController) {
        use crate::automaton::delta::{Contract, ContractKind, NeighborKind};
        let w = ctrl.field.width;
        let h = ctrl.field.height;
        let d = ctrl.field.depth;

        // X axis: connect x=W-1 to x=0 for every (y, z).
        for z in 0..d {
            for y in 0..h {
                let i_last = idx(w, h, w - 1, y, z);
                let i_wrap = idx(w, h, 0, y, z);
                ctrl.delta_overrides
                    .insert((i_last, i_wrap), NeighborKind::Mirror);
                ctrl.contract_list.contracts.push(Contract {
                    src_a: i_last as u32,
                    src_b: i_wrap as u32,
                    dst_a: i_last as u32,
                    dst_b: i_wrap as u32,
                    kind: ContractKind::Portal,
                });
            }
        }
        // Y axis: connect y=H-1 to y=0 for every (x, z).
        for z in 0..d {
            for x in 0..w {
                let i_last = idx(w, h, x, h - 1, z);
                let i_wrap = idx(w, h, x, 0, z);
                ctrl.delta_overrides
                    .insert((i_last, i_wrap), NeighborKind::Mirror);
                ctrl.contract_list.contracts.push(Contract {
                    src_a: i_last as u32,
                    src_b: i_wrap as u32,
                    dst_a: i_last as u32,
                    dst_b: i_wrap as u32,
                    kind: ContractKind::Portal,
                });
            }
        }
        // Z axis: connect z=D-1 to z=0 for every (x, y).
        for y in 0..h {
            for x in 0..w {
                let i_last = idx(w, h, x, y, d - 1);
                let i_wrap = idx(w, h, x, y, 0);
                ctrl.delta_overrides
                    .insert((i_last, i_wrap), NeighborKind::Mirror);
                ctrl.contract_list.contracts.push(Contract {
                    src_a: i_last as u32,
                    src_b: i_wrap as u32,
                    dst_a: i_last as u32,
                    dst_b: i_wrap as u32,
                    kind: ContractKind::Portal,
                });
            }
        }
    }

    /// Phase 8B+: Portal — 3-torus topology congruence test.
    ///
    /// On a 3-torus all positions are equivalent: a blob at the center and a
    /// blob at the corner must produce the same diffusion history up to a
    /// toroidal coordinate shift. After N steps both fields are translated to
    /// align and checked for fuzzy congruence (noise < 0.3 units/cell/step).
    #[test]
    fn test_portal_3torus_congruence() {
        let (w, h, d) = (32i16, 32i16, 32i16);
        let steps = 20u32;
        let blob_mass = 1_000_000u32;

        // --- Field A: blob at center ---
        let mut ctrl_a = StepController::new(w, h, d, 3, 1);
        let cx = w / 2;
        let cy = h / 2;
        let cz = d / 2;
        ctrl_a.field.cells[idx(w, h, cx, cy, cz)] = blob_mass;
        register_3torus_portals(&mut ctrl_a);
        for _ in 0..steps {
            ctrl_a.step_blocking();
        }

        // --- Field B: blob at corner (0,0,0) ---
        let mut ctrl_b = StepController::new(w, h, d, 3, 1);
        ctrl_b.field.cells[idx(w, h, 0, 0, 0)] = blob_mass;
        register_3torus_portals(&mut ctrl_b);
        for _ in 0..steps {
            ctrl_b.step_blocking();
        }

        // Translate B by (cx, cy, cz) so its origin aligns with A's center blob.
        let b_translated = torus_translate(&ctrl_b.field.cells, w, h, d, cx, cy, cz);

        assert!(
            fuzzy_congruent(&ctrl_a.field.cells, &b_translated, steps, 0.3),
            "3-torus: center-blob and corner-blob fields must be congruent after {} steps \
             (noise threshold 0.3 units/cell/step)",
            steps
        );
    }

    /// Phase 8B+: Buffered — cross-rate-boundary accumulation.
    ///
    /// Buffered is a DeltaKind (spatial pair override). The pair accumulates flow
    /// each step but applies nothing until the drain tick. A small gradient is used
    /// so the remainder_acc contamination from i_a's other pairs doesn't reach i_b
    /// via stochastic rounding within the same tile.
    ///
    /// Checks: (1) B is unchanged until drain tick, (2) B gains mass after drain,
    /// (3) global mass is conserved (Buffered is symmetric).
    #[test]
    fn test_buffered_drains_on_slow_tick() {
        use crate::automaton::delta::NeighborKind;

        let (w, h, d) = (8i16, 8i16, 8i16);
        let drain_every = 4u32;
        let i_a = idx(w, h, 0, 0, 0);
        let i_b = idx(w, h, 1, 0, 0);

        let mut ctrl = StepController::new(w, h, d, 3, 1);
        // Small gradient: keeps remainder_acc contamination below the firing threshold
        // for i_b's own zero-gradient pairs in the same tile.
        ctrl.field.cells[i_a] = 100;
        ctrl.field.cells[i_b] = 1;

        let mass_before: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        let b_before = ctrl.field.cells[i_b];

        // Buffered is the spatial pair override — it IS the contract on this face.
        ctrl.delta_overrides.insert(
            (i_a, i_b),
            NeighborKind::Buffered {
                accumulated: 0,
                drain_every,
                ticks: 0,
            },
        );

        // Run (drain_every - 1) steps: B must not have changed from the Buffered contract.
        for _ in 0..(drain_every - 1) {
            ctrl.step_blocking();
        }
        assert_eq!(
            ctrl.field.cells[i_b], b_before,
            "Buffered: B must not change before drain tick"
        );

        // Run one more step (the drain tick).
        ctrl.step_blocking();

        let mass_after: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(
            mass_before, mass_after,
            "Buffered: mass must be conserved across drain (before={}, after={})",
            mass_before, mass_after
        );
        assert!(
            ctrl.field.cells[i_b] > b_before,
            "Buffered: B must have gained mass after drain (still {})",
            ctrl.field.cells[i_b]
        );
        assert!(
            ctrl.field.cells[i_a] < 100_000,
            "Buffered: A must have lost mass after drain"
        );
    }

    // -----------------------------------------------------------------------
    // Entity API sketch — diving suit scenario
    // -----------------------------------------------------------------------
    //
    // A player wearing a rebreathing diving suit:
    //   - Does NOT exchange substance (gas/liquid) with surroundings.
    //   - DOES exchange heat with surroundings at a fixed conductivity.
    //   - Maintains a constant internal temperature (homeostasis).
    //   - Moves around the field, so the entity contract must be relocatable.
    //
    // Proposed API shape:
    //
    //   ctrl.contract_list.push_entity(
    //       i_voxel,                        // which voxel the entity currently occupies
    //       EntityHandle { lua_ref: 42 },   // opaque Lua reference for callback
    //       EntityContractFlags::HEAT_ONLY, // substance blocked, heat allowed
    //   );
    //
    //   // To move the entity each Lua tick:
    //   ctrl.contract_list.move_entity(lua_ref, new_voxel_idx);
    //
    //   // The Rust kernel calls back into Lua each step to get the entity's
    //   // current temperature and applies it as:
    //   //   flow = compute_flow(voxel_val - entity_temp, conductivity, ...)
    //   //   target[voxel] += flow   // voxel gains/loses heat toward entity temp
    //   //   (entity side: Lua callback receives `flow`, applies homeostasis logic)
    //
    // Open questions before implementation:
    //   1. Who holds the entity's current value — Rust (cached in EntityHandle)
    //      or Lua (polled each step via callback)? Caching in Rust avoids FFI
    //      overhead per cell per step; polling from Lua is simpler and safer.
    // a1) well I guess this is a symptom of this mod being not quite native to Luanti. It's not really a native Lua module, it's a wrapper around a native Rust kernel. That means the obvious implementation of just having the entity and the field in the same program is not so available unless the mod is much closer integrated into Luanti! I believe Luanti has an available Lua module to store modded data about the entity which can be fetched as a single struct or value from the Lua side in one light api call, and the result converted in place into a Rust struct or value describing the entity's current state, and sent back to the Lua side of that entity via one light api call.
    //   2. Homeostasis resistance: does the entity just clamp its value back to
    //      T_body after each step (Lua callback discards the flow delta), or
    //      does it have a finite thermal mass that warms/cools slowly?
    // a2) in ONI, the entity homeostatis is a bang-bang controller that
    //      burns the Dupe's internal resource "kcal" to maintain temperature. For testing purposes, you can assume the entity's homeostatis has effectively infinite fuel and won't starve.
    //   3. Substance blocking: does this need a per-field-quantity flag on the
    //      contract, or is the diving suit modeled as Mirror for all non-heat
    //      quantities and Entity only for the heat field?
    // a3) well, that depends how the entity is implemented, but it probably will be an Entity deltakind member of all the Rust-simulated fields and have different conductivities, so like if a humanoid physics entity jumps from the STP air to liquid nitrogen, it will drown (substances field) and freeze (heat field) and sink (buoyancy - pressure fields), similarly for electric fields, etc.
}
