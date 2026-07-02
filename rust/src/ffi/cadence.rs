//! FFI interface for cadence partition operations (Phase 9c).

use crate::automaton::incremental::StepController;
use crate::automaton::cadence::Cadence;

/// Advance cadence partition one global tick.
/// Writes firing zones into caller-supplied flat arrays (max_zones capacity).
/// Returns number of zones that fired this tick (0 = nothing stepped this tick).
/// out_zone_data layout per zone: [min_x, min_y, min_z, max_x, max_y, max_z, cadence] (7 x i16)
#[no_mangle]
pub extern "C" fn va_sc_cadence_advance(
    ctrl: *mut StepController,
    out_zone_data: *mut i16,
    max_zones: u32,
) -> u32 {
    if ctrl.is_null() || out_zone_data.is_null() {
        return 0;
    }

    unsafe {
        let ctrl = &mut *ctrl;
        let firing = ctrl.cadence_partition.advance();

        if firing.is_empty() || max_zones == 0 {
            return firing.len() as u32;
        }

        let mut count = 0;
        for (zone, cadence) in firing.iter().take(max_zones as usize) {
            let out_ptr = out_zone_data.add(count as usize * 7);
            *out_ptr = zone.min[0];
            *out_ptr.add(1) = zone.min[1];
            *out_ptr.add(2) = zone.min[2];
            *out_ptr.add(3) = zone.max[0];
            *out_ptr.add(4) = zone.max[1];
            *out_ptr.add(5) = zone.max[2];
            *out_ptr.add(6) = cadence.get() as i16;
            count += 1;
        }
        count as u32
    }
}

/// Convenience: advance one tick, then step_zones_blocking on whatever fired.
/// Returns number of zones stepped (0 = nothing fired this tick).
#[no_mangle]
pub extern "C" fn va_sc_cadence_step(ctrl: *mut StepController) -> u32 {
    if ctrl.is_null() {
        return 0;
    }

    unsafe {
        let ctrl = &mut *ctrl;
        let firing = ctrl.cadence_partition.advance();

        if !firing.is_empty() {
            ctrl.step_zones_blocking(&firing);
        }

        firing.len() as u32
    }
}

/// Enumerate all leaves of the cadence partition into a flat array.
/// out_leaf_data layout per leaf: [min_x, min_y, min_z, max_x, max_y, max_z, cadence] (7 x i16)
/// Returns the number of leaves written (capped at max_leaves).
#[no_mangle]
pub extern "C" fn va_sc_cadence_leaves(
    ctrl: *const StepController,
    out_leaf_data: *mut i16,
    max_leaves: u32,
) -> u32 {
    if ctrl.is_null() || out_leaf_data.is_null() {
        return 0;
    }

    unsafe {
        let ctrl = &*ctrl;
        let leaves = ctrl.cadence_partition.leaves();

        let mut count = 0;
        for leaf in leaves.iter().take(max_leaves as usize) {
            if let crate::automaton::cadence::CadenceNode::Leaf { region, cadence, .. } = leaf {
                let out_ptr = out_leaf_data.add(count as usize * 7);
                *out_ptr = region.min[0];
                *out_ptr.add(1) = region.min[1];
                *out_ptr.add(2) = region.min[2];
                *out_ptr.add(3) = region.max[0];
                *out_ptr.add(4) = region.max[1];
                *out_ptr.add(5) = region.max[2];
                *out_ptr.add(6) = cadence.get() as i16;
                count += 1;
            }
        }
        count as u32
    }
}

/// Bisect the leaf containing (px,py,pz) at the given axis and coord.
/// lo_cadence applies to the low side, hi_cadence to the high side.
/// Also registers Buffered contracts on the seam face-pairs (via delta_overrides).
/// Returns 0 on success, -1 on failure (e.g. point out of bounds).
#[no_mangle]
pub extern "C" fn va_sc_cadence_bisect(
    ctrl: *mut StepController,
    px: i16,
    py: i16,
    pz: i16,
    axis: u8,
    coord: i16,
    lo_cadence: u16,
    hi_cadence: u16,
) -> i32 {
    if ctrl.is_null() {
        return -1;
    }

    if lo_cadence == 0 || hi_cadence == 0 {
        return -1;
    }

    unsafe {
        let ctrl = &mut *ctrl;

        // Find the leaf containing (px,py,pz) and reject coords that would
        // produce a zero-thickness half on either side, rather than letting
        // CadenceNode::bisect create a degenerate leaf (its debug_assert for
        // this is compiled out in release builds).
        let leaves = ctrl.cadence_partition.leaves();
        let containing = leaves.iter().find_map(|leaf| {
            if let crate::automaton::cadence::CadenceNode::Leaf { region, .. } = leaf {
                if region.contains(px, py, pz) {
                    return Some(region.clone());
                }
            }
            None
        });
        let region = match containing {
            Some(r) => r,
            None => return -1,
        };
        let axis_idx = axis as usize;
        if axis_idx > 2 || coord <= region.min[axis_idx] || coord >= region.max[axis_idx] {
            return -1;
        }

        let lo_cad = match Cadence::new(lo_cadence) {
            cad => cad,
        };
        let hi_cad = match Cadence::new(hi_cadence) {
            cad => cad,
        };

        match ctrl.cadence_partition.bisect([px, py, pz], axis, coord, lo_cad, 0, hi_cad, 0) {
            Some(seam) => {
                // Register Buffered contracts on the seam face-pairs
                let pairs = seam.face_pairs(ctrl.field.width, ctrl.field.height, ctrl.field.depth);
                for (lo_idx, hi_idx) in pairs {
                    // Insert NeighborKind::Buffered{drain_every} into delta_overrides
                    // For now, use drain_every = cadence (simplest strategy)
                    let drain_every = lo_cad.get().min(hi_cad.get()) as u32;
                    use crate::automaton::delta::NeighborKind;
                    ctrl.delta_overrides.insert(
                        (lo_idx, hi_idx),
                        NeighborKind::Buffered {
                            accumulated: 0,
                            drain_every,
                            ticks: 0,
                        },
                    );
                }
                0
            }
            None => -1,
        }
    }
}

/// Poll the merge of the two leaves containing null_point and alt_point.
/// Call once per global tick (after va_sc_cadence_step) until it returns 1.
/// Returns: 1 = merge complete (seam dissolved), 0 = still syncing, -1 = error.
#[no_mangle]
pub extern "C" fn va_sc_cadence_merge_poll(
    ctrl: *mut StepController,
    null_x: i16,
    null_y: i16,
    null_z: i16,
    alt_x: i16,
    alt_y: i16,
    alt_z: i16,
) -> i32 {
    if ctrl.is_null() {
        return -1;
    }

    unsafe {
        let ctrl = &mut *ctrl;
        use crate::automaton::cadence::SyncStatus;

        match ctrl.cadence_partition.merge([null_x, null_y, null_z], [alt_x, alt_y, alt_z]) {
            SyncStatus::Done(seam) => {
                // Deregister the Buffered contracts on the dissolved seam
                let pairs = seam.face_pairs(ctrl.field.width, ctrl.field.height, ctrl.field.depth);
                for (lo_idx, hi_idx) in pairs {
                    ctrl.delta_overrides.remove(&(lo_idx, hi_idx));
                }
                1
            }
            SyncStatus::Syncing => 0,
        }
    }
}

/// Return the cadence period of the zone containing (x,y,z). Returns 0 on error.
#[no_mangle]
pub extern "C" fn va_sc_cadence_lookup(
    ctrl: *const StepController,
    x: i16,
    y: i16,
    z: i16,
) -> u16 {
    if ctrl.is_null() {
        return 0;
    }

    unsafe {
        let ctrl = &*ctrl;
        ctrl.cadence_partition.lookup_cadence(x, y, z).get()
    }
}

/// Return the current global_tick counter.
#[no_mangle]
pub extern "C" fn va_sc_global_tick(ctrl: *const StepController) -> u64 {
    if ctrl.is_null() {
        return 0;
    }

    unsafe {
        let ctrl = &*ctrl;
        ctrl.global_tick
    }
}

/// Create an Infinity contract at the given field coordinates with target_value.
/// The contract couples the cell at (x,y,z) to a virtual cell held at target_value.
/// Returns 0 on success, -1 on error (e.g. out of bounds).
#[no_mangle]
pub extern "C" fn va_sc_infinity_create(
    ctrl: *mut StepController,
    x: i16,
    y: i16,
    z: i16,
    target_value: u32,
) -> i32 {
    if ctrl.is_null() {
        return -1;
    }

    unsafe {
        let ctrl = &mut *ctrl;

        // Validate coordinates are in field bounds
        if x < 0 || x >= ctrl.field.width || y < 0 || y >= ctrl.field.height || z < 0 || z >= ctrl.field.depth {
            return -1;
        }

        // Compute cell index from coordinates
        let index = x as u32 + (y as u32) * (ctrl.field.width as u32) + (z as u32) * (ctrl.field.width as u32) * (ctrl.field.height as u32);

        use crate::automaton::delta::{Contract, ContractKind};

        // Refuse to stack a second Infinity contract on the same cell instead of
        // silently pushing a duplicate that would fight the existing one for
        // control of the cell's value.
        let already_exists = ctrl.contract_list.contracts.iter().any(|c| {
            c.src_a == index && matches!(c.kind, ContractKind::Infinity { .. })
        });
        if already_exists {
            return -1;
        }

        let contract = Contract {
            src_a: index,
            src_b: 0,
            // apply_one_sided writes the computed flow into target[dst_a], so
            // this must be the same cell the gradient was measured from.
            dst_a: index,
            dst_b: 0,
            kind: ContractKind::Infinity { target_value, consumed: 0 },
        };

        ctrl.contract_list.contracts.push(contract);
        0
    }
}

/// Destroy/clear the Infinity contract at the given field coordinates.
/// Returns 0 on success, -1 on error (contract not found or out of bounds).
#[no_mangle]
pub extern "C" fn va_sc_infinity_destroy(
    ctrl: *mut StepController,
    x: i16,
    y: i16,
    z: i16,
) -> i32 {
    if ctrl.is_null() {
        return -1;
    }

    unsafe {
        let ctrl = &mut *ctrl;

        // Validate coordinates are in field bounds
        if x < 0 || x >= ctrl.field.width || y < 0 || y >= ctrl.field.height || z < 0 || z >= ctrl.field.depth {
            return -1;
        }

        // Compute cell index from coordinates
        let index = x as u32 + (y as u32) * (ctrl.field.width as u32) + (z as u32) * (ctrl.field.width as u32) * (ctrl.field.height as u32);

        use crate::automaton::delta::ContractKind;

        // Find and remove the Infinity contract for this cell
        let initial_len = ctrl.contract_list.contracts.len();
        ctrl.contract_list.contracts.retain(|c| {
            if c.src_a == index {
                if let ContractKind::Infinity { .. } = c.kind {
                    return false; // Remove this contract
                }
            }
            true // Keep this contract
        });

        if ctrl.contract_list.contracts.len() < initial_len {
            0
        } else {
            -1 // Contract not found
        }
    }
}
