//! Diffusion physics kernel: tile layout, flow computation, and per-tile stepping.
//!
//! Core invariant: Snapshot double-buffer preserves field_step_fused correctness.
//! All reads from immutable generation-N snapshot, all writes to generation-N+1 buffer.
//! Tile processing order doesn't affect result (commutative accumulation across tiles).

use std::sync::atomic::AtomicUsize;

use crate::automaton::delta::{ContractKind, ContractList, NeighborOverrides};

/// Apply flow between one real cell and one virtual neighbor held at `virtual_value`.
/// The real cell loses flow (or gains if gradient is negative). Mass is not conserved:
/// the virtual side is a reservoir, not a grid cell. `consumed` accumulates signed flow
/// (positive = mass leaves the real cell). Remainder is reset after each call to prevent
/// contamination of subsequent entries in the same post-pass.
#[inline(always)]
fn apply_one_sided(
    source: &[u32],
    target: &mut [u32],
    src_a: u32,
    dst_a: u32,
    virtual_value: i64,
    consumed: &mut i64,
    conductivity: i64,
    divisor: i64,
    dt: i64,
    remainder_acc: &mut i64,
) {
    let gradient = source[src_a as usize] as i64 - virtual_value;
    let flow = compute_flow(gradient, conductivity, divisor, dt, remainder_acc);
    *consumed += flow;
    target[dst_a as usize] = ((target[dst_a as usize] as i64) - flow) as u32;
    *remainder_acc = 0;
}

pub const MAPBLOCK_SIZE: i16 = 16;

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
    pub delta_overrides: NeighborOverrides,

    /// Per-cell flag: true if this cell owns at least one override pair.
    /// Checked before the hash lookup to keep the modal fast path branch-free.
    pub cell_has_override: Vec<bool>,

    /// Time step in global ticks for this step. 1 for full-field steps; equals the
    /// zone's cadence for zone-selective steps. Scales flow proportionally so the
    /// physical time constant is preserved across different cadences.
    pub dt: i64,
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
pub fn compute_flow(
    gradient: i64,
    conductivity: i64,
    divisor: i64,
    dt: i64,
    remainder_acc: &mut i64,
) -> i64 {
    debug_assert!(dt >= 1, "dt must be at least 1 global tick");
    // Stability: conductivity * dt must be less than divisor to guarantee no cell
    // loses more than its entire value in one step. Violation causes u32 underflow
    // (wraps to near-u32::MAX), which has been observed to destroy conservation.
    debug_assert!(
        conductivity * dt < divisor,
        "dt={} is too large: conductivity * dt ({}) >= divisor ({}); \
         this step size violates the stability bound and will cause underflow. \
         Reduce cadence or increase diffusion_rate (max safe dt ≈ {}).",
        dt, conductivity * dt, divisor, divisor / conductivity,
    );
    let product = gradient * conductivity * dt;
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

/// Resolve the flow for a spatial pair, checking the override map when `check` is true.
#[inline(always)]
fn resolve_pair(
    overrides: &mut NeighborOverrides,
    check: bool,
    idx_a: usize,
    idx_b: usize,
    gradient: i64,
    conductivity: i64,
    divisor: i64,
    dt: i64,
    remainder_acc: &mut i64,
) -> i64 {
    if check {
        if let Some(kind) = overrides.get_mut(&(idx_a, idx_b)) {
            return kind.apply(gradient, conductivity, divisor, remainder_acc,
                |g, c, d, acc| compute_flow(g, c, d, dt, acc));
        }
    }
    compute_flow(gradient, conductivity, divisor, dt, remainder_acc)
}

/// Apply a resolved flow symmetrically to both sides of a spatial pair.
#[inline(always)]
fn apply_pair(target: &mut [u32], idx_a: usize, idx_b: usize, flow: i64) {
    target[idx_a] = ((target[idx_a] as i64) - flow) as u32;
    target[idx_b] = ((target[idx_b] as i64) + flow) as u32;
}

/// Process a single 16³ tile. Computes phase C (diffusion flows).
/// Formula: ΔΦ = (ΔV * C_mat) / (N_base * S_face * 2^shift * 2^16)
/// Stability: divisor >= 7 ensures no cell loses more than 1/7 of its value per step.
pub fn process_tile(step: &mut IncrementalStep, tile: TileCoord) {
    let x_start = tile.tx as i16 * MAPBLOCK_SIZE;
    let y_start = tile.ty as i16 * MAPBLOCK_SIZE;
    let z_start = tile.tz as i16 * MAPBLOCK_SIZE;

    let x_end = (x_start + MAPBLOCK_SIZE).min(step.width);
    let y_end = (y_start + MAPBLOCK_SIZE).min(step.height);
    let z_end = (z_start + MAPBLOCK_SIZE).min(step.depth);

    let shift = step.diffusion_rate as u32;
    // Conductivity is fixed at ~1.0 (fully conductive, scaled by 2^16)
    let conductivity = 65535i64;
    let divisor = (7i64 << shift) << 16;
    let dt = step.dt;
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
                    let flow = resolve_pair(
                        &mut step.delta_overrides,
                        check_override,
                        idx_a,
                        idx_b,
                        gradient,
                        conductivity,
                        divisor,
                        dt,
                        &mut remainder_acc,
                    );
                    apply_pair(&mut step.target, idx_a, idx_b, flow);
                } else {
                    let flow = compute_flow(0, conductivity, divisor, dt, &mut remainder_acc);
                    step.target[idx_a] = ((step.target[idx_a] as i64) - flow) as u32;
                }

                // Y-axis pair: (x, y, z) with (x, y+1, z) or mirror at boundary
                if y + 1 < step.height {
                    let idx_b = field_index(step, x, y + 1, z);
                    let gradient = step.source[idx_a] as i64 - step.source[idx_b] as i64;
                    let flow = resolve_pair(
                        &mut step.delta_overrides,
                        check_override,
                        idx_a,
                        idx_b,
                        gradient,
                        conductivity,
                        divisor,
                        dt,
                        &mut remainder_acc,
                    );
                    apply_pair(&mut step.target, idx_a, idx_b, flow);
                } else {
                    let flow = compute_flow(0, conductivity, divisor, dt, &mut remainder_acc);
                    step.target[idx_a] = ((step.target[idx_a] as i64) - flow) as u32;
                }

                // Z-axis pair: (x, y, z) with (x, y, z+1) or mirror at boundary
                if z + 1 < step.depth {
                    let idx_b = field_index(step, x, y, z + 1);
                    let gradient = step.source[idx_a] as i64 - step.source[idx_b] as i64;
                    let flow = resolve_pair(
                        &mut step.delta_overrides,
                        check_override,
                        idx_a,
                        idx_b,
                        gradient,
                        conductivity,
                        divisor,
                        dt,
                        &mut remainder_acc,
                    );
                    apply_pair(&mut step.target, idx_a, idx_b, flow);
                } else {
                    let flow = compute_flow(0, conductivity, divisor, dt, &mut remainder_acc);
                    step.target[idx_a] = ((step.target[idx_a] as i64) - flow) as u32;
                }
            }
        }
    }
}

/// Process all ContractList entries after the tile pass, using the frozen source snapshot.
/// Handles Portal, Void, and (stubs for) Remote and Entity.
pub fn process_contract_list(
    source: &[u32],
    target: &mut [u32],
    contract_list: &mut ContractList,
    diffusion_rate: u8,
    dt: i64,
) {
    let shift = diffusion_rate as u32;
    let conductivity = 65535i64;
    let divisor = (7i64 << shift) << 16;
    let mut remainder_acc = 0i64;

    for contract in &mut contract_list.contracts {
        match &mut contract.kind {
            ContractKind::Portal => {
                let gradient =
                    source[contract.src_a as usize] as i64 - source[contract.src_b as usize] as i64;
                let flow = compute_flow(gradient, conductivity, divisor, dt, &mut remainder_acc);
                apply_pair(
                    target,
                    contract.src_a as usize,
                    contract.src_b as usize,
                    flow,
                );
            }
            ContractKind::Void { consumed } => {
                apply_one_sided(
                    source,
                    target,
                    contract.src_a,
                    contract.dst_a,
                    0,
                    consumed,
                    conductivity,
                    divisor,
                    dt,
                    &mut remainder_acc,
                );
            }
            ContractKind::Infinity {
                target_value,
                consumed,
            } => {
                apply_one_sided(
                    source,
                    target,
                    contract.src_a,
                    contract.dst_a,
                    *target_value as i64,
                    consumed,
                    conductivity,
                    divisor,
                    dt,
                    &mut remainder_acc,
                );
            }
            ContractKind::Remote | ContractKind::Entity => {
                // Not yet implemented. Will use apply_one_sided with virtual_value sourced
                // from RemoteEndpoint::cached_value or EntityHandle lua_ref respectively.
                // they might also need to copy implementation details from Buffer since lua ticks are not necessarily sim steps, and the remote server might be lagging or behind or ticking at a different rate than the local server.
            }
        }
    }
}
