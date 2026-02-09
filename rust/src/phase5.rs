// Phase 5: Bidirectional Sync
//
// Provides FFI functions for importing cell data from the world back into the automaton.

use crate::state::State;

/// Imports a rectangular region of cells from a flat buffer into the automaton.
///
/// Buffer layout: z,y,x order (same as va_extract_region for symmetry)
/// Input values are normalized: 0 = dead, non-zero = alive
///
/// # Safety
/// - `ptr` must be a valid State pointer from va_create()
/// - `in_buf` must point to at least (max-min) volume bytes
/// - Coordinates will be clamped to grid bounds
///
/// # Returns
/// Number of bytes read from buffer, or 0 on error (null ptr/buffer)
#[no_mangle]
pub unsafe extern "C" fn va_import_region(
    ptr: *mut State,
    in_buf: *const u8,
    min_x: i16,
    min_y: i16,
    min_z: i16,
    max_x: i16,
    max_y: i16,
    max_z: i16,
) -> u64 {
    // Null checks
    if ptr.is_null() || in_buf.is_null() {
        return 0;
    }

    let state = &mut *ptr;

    // Clamp coordinates to grid bounds
    let min_x = min_x.max(0).min(state.width);
    let min_y = min_y.max(0).min(state.height);
    let min_z = min_z.max(0).min(state.depth);
    let max_x = max_x.max(0).min(state.width);
    let max_y = max_y.max(0).min(state.height);
    let max_z = max_z.max(0).min(state.depth);

    // Handle empty or inverted regions
    if min_x >= max_x || min_y >= max_y || min_z >= max_z {
        return 0;
    }

    let mut offset = 0;

    // Iterate in z,y,x order (matches va_extract_region)
    for z in min_z..max_z {
        for y in min_y..max_y {
            for x in min_x..max_x {
                let value = *in_buf.add(offset);
                let normalized = if value == 0 { 0 } else { 1 };

                let idx = state.index(x, y, z);
                state.cells[idx] = normalized;

                offset += 1;
            }
        }
    }

    offset as u64
}
