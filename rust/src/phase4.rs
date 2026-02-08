//! Phase 4: Visualize
//!
//! Region extraction for visualization in Luanti.

use crate::state::State;

/// Extracts a rectangular region of cells into a flat output buffer.
/// The buffer is filled in z,y,x order (z changes slowest, x changes fastest).
/// Safety: ptr must be a valid pointer to a State with a grid.
///         out_buf must point to a buffer with at least (max_x-min_x) * (max_y-min_y) * (max_z-min_z) bytes.
/// Returns the number of bytes written.
#[no_mangle]
pub unsafe extern "C" fn va_extract_region(
    ptr: *const State,
    out_buf: *mut u8,
    min_x: i16,
    min_y: i16,
    min_z: i16,
    max_x: i16,
    max_y: i16,
    max_z: i16,
) -> u64 {
    if ptr.is_null() || out_buf.is_null() {
        return 0;
    }

    let state = &*ptr;
    if state.cells.is_empty() {
        return 0;
    }

    // Clamp coordinates to grid bounds
    let min_x = min_x.max(0).min(state.width);
    let min_y = min_y.max(0).min(state.height);
    let min_z = min_z.max(0).min(state.depth);
    let max_x = max_x.max(0).min(state.width);
    let max_y = max_y.max(0).min(state.height);
    let max_z = max_z.max(0).min(state.depth);

    // Check for empty region
    if min_x >= max_x || min_y >= max_y || min_z >= max_z {
        return 0;
    }

    let width = (max_x - min_x) as usize;
    let height = (max_y - min_y) as usize;
    let depth = (max_z - min_z) as usize;

    // Create a slice for safe writing
    let out_slice = std::slice::from_raw_parts_mut(out_buf, width * height * depth);

    let mut offset = 0;
    for z in min_z..max_z {
        for y in min_y..max_y {
            for x in min_x..max_x {
                let idx = state.index(x, y, z);
                out_slice[offset] = state.cells[idx];
                offset += 1;
            }
        }
    }

    (width * height * depth) as u64
}
