//! Diffusion physics kernel: tile layout, flow computation, and per-tile stepping.
//!
//! Core invariant: Snapshot double-buffer preserves field_step_fused correctness.
//! All reads from immutable generation-N snapshot, all writes to generation-N+1 buffer.
//! Tile processing order doesn't affect result (commutative accumulation across tiles).

use std::sync::atomic::AtomicUsize;

use crate::automaton::delta::{ContractKind, ContractList, DeltaKind, DeltaOverrides};

pub const TILE_SIZE: i16 = 16;

/// 3D tile coordinate.
#[derive(Clone, Copy, Debug)]
pub struct TileCoord {
    pub tx: u8,
    pub ty: u8,
    pub tz: u8,
}

/// Snapshot of field state being stepped. Owned by the scheduler during a step.
pub struct IncrementalStep {
    /// Immutable snapshot of cells at generation N (read-only during step).
    pub source: Vec<u32>,

    /// Accumulating output for generation N+1 (written by tile processors).
    pub target: Vec<u32>,

    /// Ordered list of tile coordinates to process, in Morton order.
    pub tile_queue: Vec<TileCoord>,

    /// Index into tile_queue: next tile to process. Atomic for future Rayon use.
    pub next_tile: AtomicUsize,

    /// Total number of tiles.
    pub total_tiles: usize,

    /// The generation number this step will produce (field.generation + 1).
    pub target_generation: u64,

    /// Field dimensions (cached for tile processing).
    pub width: i16,
    pub height: i16,
    pub depth: i16,

    /// Diffusion rate (cached).
    pub diffusion_rate: u8,

    /// Sparse per-pair contract overrides. Key: (owner_idx, neighbor_idx).
    /// Empty for fully-modal fields.
    pub delta_overrides: DeltaOverrides,

    /// Per-cell flag: true if this cell owns at least one override pair.
    /// Checked before the hash lookup to keep the modal fast path branch-free.
    pub cell_has_override: Vec<bool>,
}

/// Interleave bits of x, y, z to produce a Morton code.
/// For tile indices up to 255 (supports fields up to 4096 per axis), u32 output suffices.
fn morton_encode(x: u8, y: u8, z: u8) -> u32 {
    fn spread_bits(v: u8) -> u32 {
        let mut x = v as u32;
        x = (x | (x << 16)) & 0x030000FF;
        x = (x | (x << 8)) & 0x0300F00F;
        x = (x | (x << 4)) & 0x030C30C3;
        x = (x | (x << 2)) & 0x09249249;
        x
    }
    spread_bits(x) | (spread_bits(y) << 1) | (spread_bits(z) << 2)
}
// TODO: verify that a 1500x1500x500 field is valid

/// Build a list of all tile coordinates, sorted by Morton code.
pub fn build_tile_queue(tiles_x: u8, tiles_y: u8, tiles_z: u8) -> Vec<TileCoord> {
    let mut tiles: Vec<(u32, TileCoord)> = Vec::new();

    for tz in 0..tiles_z {
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let morton = morton_encode(tx, ty, tz);
                let coord = TileCoord { tx, ty, tz };
                tiles.push((morton, coord));
            }
        }
    }

    tiles.sort_by_key(|&(morton, _)| morton);
    tiles.into_iter().map(|(_, coord)| coord).collect()
}

/// Compute linear index in field cells using row-major z/y/x layout.
#[inline]
fn field_index(field: &IncrementalStep, x: i16, y: i16, z: i16) -> usize {
    z as usize * field.height as usize * field.width as usize
        + y as usize * field.width as usize
        + x as usize
}

/// Compute diffusion flow: ΔΦ = (ΔV * C_mat) / (N_base * S_face * 2^shift * 2^16)
/// Uses stochastic rounding via remainder accumulator for realistic small-scale diffusion.
///
/// Known issue: vacuum decay. The remainder accumulator is shared across all
/// cells in a tile. When it builds up from non-zero gradients and then encounters a
/// zero-gradient pair (two adjacent cells both at zero), stochastic rounding can produce a
/// flow of ±1 between them. The unsigned wrapping cast in process_tile then turns a -1 into
/// u32::MAX (2^32 - 1), creating massive spontaneous mass. This mirrors quantum vacuum
/// fluctuations: a true zero-energy state is physically impossible, and achieving one in-game
/// triggers an energy release. To be addressed in a future physics engine revision.
#[inline]
pub fn compute_flow(gradient: i64, conductivity: i64, divisor: i64, remainder_acc: &mut i64) -> i64 {
    let product = gradient * conductivity;
    let flow_truncated = product / divisor;
    let remainder = product % divisor;

    *remainder_acc += remainder.abs();

    if *remainder_acc >= divisor {
        *remainder_acc -= divisor;
        if gradient >= 0 {
            flow_truncated + 1
        } else {
            flow_truncated - 1
        }
    } else {
        flow_truncated
    }
}

/// Resolve the flow and whether to suppress the b-side write for a spatial pair.
/// Checks the override map when `check` is true; falls back to modal otherwise.
/// Returns `(flow, suppress_b_write)`.
#[inline(always)]
fn resolve_pair(
    overrides: &mut DeltaOverrides,
    check: bool,
    idx_a: usize,
    idx_b: usize,
    gradient: i64,
    conductivity: i64,
    divisor: i64,
    remainder_acc: &mut i64,
) -> (i64, bool) {
    if check {
        if let Some(kind) = overrides.get_mut(&(idx_a, idx_b)) {
            let suppress = matches!(kind, DeltaKind::Void { .. });
            return (kind.apply(gradient, conductivity, divisor, remainder_acc, compute_flow), suppress);
        }
    }
    (compute_flow(gradient, conductivity, divisor, remainder_acc), false)
}

/// Apply a resolved flow to the target buffer. Suppresses the b-side write for Void.
#[inline(always)]
fn apply_pair(target: &mut [u32], idx_a: usize, idx_b: usize, flow: i64, suppress_b: bool) {
    target[idx_a] = ((target[idx_a] as i64) - flow) as u32;
    if !suppress_b {
        target[idx_b] = ((target[idx_b] as i64) + flow) as u32;
    }
}

/// Process a single 16³ tile. Computes phase C (diffusion flows).
/// Formula: ΔΦ = (ΔV * C_mat) / (N_base * S_face * 2^shift * 2^16)
/// Stability: divisor >= 7 ensures no cell loses more than 1/7 of its value per step.
pub fn process_tile(step: &mut IncrementalStep, tile: TileCoord) {
    let x_start = tile.tx as i16 * TILE_SIZE;
    let y_start = tile.ty as i16 * TILE_SIZE;
    let z_start = tile.tz as i16 * TILE_SIZE;

    let x_end = (x_start + TILE_SIZE).min(step.width);
    let y_end = (y_start + TILE_SIZE).min(step.height);
    let z_end = (z_start + TILE_SIZE).min(step.depth);

    let shift = step.diffusion_rate as u32;
    // Conductivity is fixed at ~1.0 (fully conductive, scaled by 2^16)
    let conductivity = 65535i64;
    let divisor = (7i64 << shift) << 16;
    let mut remainder_acc = 0i64;

    // Phase A: Consume deltas (no-op for current diffusion)
    // Future hook: consume persistent cross-generation deltas

    // Phase B: Update element state (no-op for current diffusion)
    // Future hook: multi-phase fluid dynamics, texture changes

    // Phase C: Compute and apply diffusion flows
    // Owner-writes-positive: cell (x, y, z) owns the pair with (x+1, y, z), (x, y+1, z), (x, y, z+1)
    // This prevents double-counting at tile boundaries.

    for z in z_start..z_end {
        for y in y_start..y_end {
            for x in x_start..x_end {
                let idx_a = field_index(step, x, y, z);
                let check_override = step.cell_has_override[idx_a];

                // X-axis pair: (x, y, z) with (x+1, y, z) or mirror at boundary
                if x + 1 < step.width {
                    let idx_b = field_index(step, x + 1, y, z);
                    let gradient = step.source[idx_a] as i64 - step.source[idx_b] as i64;
                    let (flow, suppress_b) = resolve_pair(&mut step.delta_overrides, check_override, idx_a, idx_b, gradient, conductivity, divisor, &mut remainder_acc);
                    apply_pair(&mut step.target, idx_a, idx_b, flow, suppress_b);
                } else {
                    let flow = compute_flow(0, conductivity, divisor, &mut remainder_acc);
                    step.target[idx_a] = ((step.target[idx_a] as i64) - flow) as u32;
                }

                // Y-axis pair: (x, y, z) with (x, y+1, z) or mirror at boundary
                if y + 1 < step.height {
                    let idx_b = field_index(step, x, y + 1, z);
                    let gradient = step.source[idx_a] as i64 - step.source[idx_b] as i64;
                    let (flow, suppress_b) = resolve_pair(&mut step.delta_overrides, check_override, idx_a, idx_b, gradient, conductivity, divisor, &mut remainder_acc);
                    apply_pair(&mut step.target, idx_a, idx_b, flow, suppress_b);
                } else {
                    let flow = compute_flow(0, conductivity, divisor, &mut remainder_acc);
                    step.target[idx_a] = ((step.target[idx_a] as i64) - flow) as u32;
                }

                // Z-axis pair: (x, y, z) with (x, y, z+1) or mirror at boundary
                if z + 1 < step.depth {
                    let idx_b = field_index(step, x, y, z + 1);
                    let gradient = step.source[idx_a] as i64 - step.source[idx_b] as i64;
                    let (flow, suppress_b) = resolve_pair(&mut step.delta_overrides, check_override, idx_a, idx_b, gradient, conductivity, divisor, &mut remainder_acc);
                    apply_pair(&mut step.target, idx_a, idx_b, flow, suppress_b);
                } else {
                    let flow = compute_flow(0, conductivity, divisor, &mut remainder_acc);
                    step.target[idx_a] = ((step.target[idx_a] as i64) - flow) as u32;
                }
            }
        }
    }
}

/// Process all Portal entries in the override map.
/// Portal pairs are non-spatial (boundary-spanning or otherwise non-adjacent),
/// so the tile pass never encounters them. This pass runs after all tiles complete,
/// reading from the frozen source snapshot and writing to the target buffer.
pub fn process_portal_overrides(step: &mut IncrementalStep) {
    let shift = step.diffusion_rate as u32;
    let conductivity = 65535i64;
    let divisor = (7i64 << shift) << 16;
    let mut remainder_acc = 0i64;

    for (&(idx_a, idx_b), kind) in step.delta_overrides.iter() {
        if matches!(kind, DeltaKind::Portal) {
            let gradient = step.source[idx_a] as i64 - step.source[idx_b] as i64;
            let flow = compute_flow(gradient, conductivity, divisor, &mut remainder_acc);
            apply_pair(&mut step.target, idx_a, idx_b, flow, false);
        }
    }
}

/// Process all ContractList entries after the tile pass.
/// Currently handles Buffered contracts; other kinds will be added as implemented.
pub fn process_contract_list(source: &[u32], target: &mut [u32], contract_list: &mut ContractList, diffusion_rate: u8) {
    let shift = diffusion_rate as u32;
    let conductivity = 65535i64;
    let divisor = (7i64 << shift) << 16;
    let mut remainder_acc = 0i64;

    for contract in &mut contract_list.contracts {
        match &mut contract.kind {
            ContractKind::Buffered { accumulated, drain_every, ticks } => {
                let gradient = source[contract.src_a as usize] as i64 - source[contract.src_b as usize] as i64;
                let flow = compute_flow(gradient, conductivity, divisor, &mut remainder_acc);
                *accumulated += flow;
                *ticks += 1;
                if *ticks >= *drain_every {
                    let drained = *accumulated;
                    *accumulated = 0;
                    *ticks = 0;
                    apply_pair(target, contract.dst_a as usize, contract.dst_b as usize, drained, false);
                }
            }
            _ => {}
        }
    }
}
