//! Region extraction and import operations.

use super::grid::index_of;
use crate::state::State;

/// Extract a rectangular region from the grid into a flat buffer.
///
/// # Layout
/// The buffer is filled in z,y,x order (z changes slowest, x changes fastest).
/// This order matches the order used by `va_import_region` for symmetry.
///
/// # Returns
/// Number of bytes written to the buffer, or 0 on error.
pub fn extract_region(
    state: &State,
    out_buf: &mut [u8],
    min_x: i16,
    min_y: i16,
    min_z: i16,
    max_x: i16,
    max_y: i16,
    max_z: i16,
) -> u64 {
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
    let total_size = width * height * depth;

    // Ensure buffer is large enough
    if out_buf.len() < total_size {
        return 0;
    }

    let mut offset = 0;
    for z in min_z..max_z {
        for y in min_y..max_y {
            for x in min_x..max_x {
                let idx = index_of(state, x, y, z);
                out_buf[offset] = state.cells[idx];
                offset += 1;
            }
        }
    }

    offset as u64
}

/// Import a rectangular region from a flat buffer into the grid.
///
/// # Layout
/// The buffer is expected to be in z,y,x order (matching `extract_region`).
/// Input values are normalized: 0 = dead, any non-zero = alive.
///
/// # Returns
/// Number of bytes read from the buffer, or 0 on error.
pub fn import_region(
    state: &mut State,
    in_buf: &[u8],
    min_x: i16,
    min_y: i16,
    min_z: i16,
    max_x: i16,
    max_y: i16,
    max_z: i16,
) -> u64 {
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

    let width = (max_x - min_x) as usize;
    let height = (max_y - min_y) as usize;
    let depth = (max_z - min_z) as usize;
    let total_size = width * height * depth;

    // Ensure buffer has enough data
    if in_buf.len() < total_size {
        return 0;
    }

    let mut offset = 0;
    for z in min_z..max_z {
        for y in min_y..max_y {
            for x in min_x..max_x {
                let value = in_buf[offset];
                let normalized = if value == 0 { 0 } else { 1 };

                let idx = index_of(state, x, y, z);
                state.cells[idx] = normalized;

                offset += 1;
            }
        }
    }

    offset as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automaton::grid::create_grid;

    #[test]
    fn test_extract_region_basic() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 8, 8, 8);

        // Set some cells alive
        let idx1 = index_of(&state, 2, 2, 2);
        state.cells[idx1] = 1;

        let idx2 = index_of(&state, 3, 2, 2);
        state.cells[idx2] = 1;

        let idx3 = index_of(&state, 2, 3, 2);
        state.cells[idx3] = 1;

        // Extract a 4x4x4 region
        let mut buffer = vec![0u8; 64];
        let bytes_written = extract_region(&state, &mut buffer, 2, 2, 2, 6, 6, 6);

        assert_eq!(bytes_written, 64);

        // Verify extracted cells
        assert_eq!(buffer[0], 1); // (2,2,2)
        assert_eq!(buffer[1], 1); // (3,2,2)
        assert_eq!(buffer[4], 1); // (2,3,2)
    }

    #[test]
    fn test_extract_region_full_grid() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 4, 4, 4);

        // Set all cells alive
        for cell in &mut state.cells {
            *cell = 1;
        }

        let mut buffer = vec![0u8; 64];
        let bytes_written = extract_region(&state, &mut buffer, 0, 0, 0, 4, 4, 4);

        assert_eq!(bytes_written, 64);
        assert!(buffer.iter().all(|&c| c == 1));
    }

    #[test]
    fn test_extract_region_empty() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 4, 4, 4);

        let mut buffer = vec![0u8; 64];
        let bytes_written = extract_region(&state, &mut buffer, 0, 0, 0, 4, 4, 4);

        assert_eq!(bytes_written, 64);
        assert!(buffer.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_extract_region_out_of_bounds() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 4, 4, 4);

        let mut buffer = vec![0u8; 512];
        let bytes_written = extract_region(&state, &mut buffer, -2, -2, -2, 10, 10, 10);

        // Should be clamped to 4x4x4
        assert_eq!(bytes_written, 64);
    }

    #[test]
    fn test_import_region_basic() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 8, 8, 8);

        let mut buffer = vec![0u8; 64];
        buffer[0] = 1;
        buffer[1] = 1;
        buffer[4] = 1;

        let bytes_read = import_region(&mut state, &buffer, 2, 2, 2, 6, 6, 6);

        assert_eq!(bytes_read, 64);
        assert_eq!(state.cells[index_of(&state, 2, 2, 2)], 1);
        assert_eq!(state.cells[index_of(&state, 3, 2, 2)], 1);
        assert_eq!(state.cells[index_of(&state, 2, 3, 2)], 1);
    }

    #[test]
    fn test_import_region_normalization() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 4, 4, 4);

        // Buffer with various values
        let buffer = vec![
            0, 1, 5, 255, // Row 0
            128, 2, 0, 1, // Row 1
            0, 0, 0, 0, // Row 2
            0, 0, 0, 0, // Row 3
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // Rest (z=1,2,3)
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        import_region(&mut state, &buffer, 0, 0, 0, 4, 4, 4);

        assert_eq!(state.cells[index_of(&state, 0, 0, 0)], 0);
        assert_eq!(state.cells[index_of(&state, 1, 0, 0)], 1);
        assert_eq!(state.cells[index_of(&state, 2, 0, 0)], 1);
        assert_eq!(state.cells[index_of(&state, 3, 0, 0)], 1);
        assert_eq!(state.cells[index_of(&state, 0, 1, 0)], 1);
    }

    #[test]
    fn test_extract_import_symmetry() {
        let mut state1 = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state1, 8, 8, 8);

        // Set some cells
        let idx1 = index_of(&state1, 2, 2, 2);
        state1.cells[idx1] = 1;

        let idx2 = index_of(&state1, 3, 3, 3);
        state1.cells[idx2] = 1;

        // Extract region
        let mut extract_buffer = vec![0u8; 64];
        extract_region(&state1, &mut extract_buffer, 0, 0, 0, 4, 4, 4);

        // Create new state and import
        let mut state2 = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state2, 8, 8, 8);
        import_region(&mut state2, &extract_buffer, 0, 0, 0, 4, 4, 4);

        // Should match within the region
        assert_eq!(
            state1.cells[index_of(&state1, 2, 2, 2)],
            state2.cells[index_of(&state2, 2, 2, 2)]
        );
        assert_eq!(
            state1.cells[index_of(&state1, 3, 3, 3)],
            state2.cells[index_of(&state2, 3, 3, 3)]
        );
    }
}
