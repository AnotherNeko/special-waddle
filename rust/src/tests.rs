#[cfg(test)]
mod tests {
    use crate::phase1::va_add;
    use crate::phase2::{va_create, va_destroy, va_get_generation};
    use crate::phase3::{va_create_grid, va_get_cell, va_set_cell, va_step};
    use crate::phase4::va_extract_region;
    use std::ptr;

    #[test]
    fn test_va_add() {
        assert_eq!(va_add(2, 3), 5);
        assert_eq!(va_add(-1, 1), 0);
        assert_eq!(va_add(0, 0), 0);
    }

    #[test]
    fn test_phase2_lifecycle() {
        unsafe {
            // Create state
            let state = va_create();
            assert!(!state.is_null(), "va_create() should not return null");

            // Check initial generation
            let gen = va_get_generation(state);
            assert_eq!(gen, 0, "Initial generation should be 0");

            // Destroy state (should not crash)
            va_destroy(state);

            // Test null pointer handling
            assert_eq!(va_get_generation(ptr::null()), 0);
            va_destroy(ptr::null_mut()); // Should not crash
        }
    }

    #[test]
    fn test_phase3_grid_creation() {
        unsafe {
            let state = va_create();
            assert!(!state.is_null());

            // Create an 8x8x8 grid
            let result = va_create_grid(state, 8, 8, 8);
            assert_eq!(result, 0, "Grid creation should succeed");

            // Check that all cells start dead
            for z in 0i16..8 {
                for y in 0i16..8 {
                    for x in 0i16..8 {
                        assert_eq!(va_get_cell(state, x, y, z), 0);
                    }
                }
            }

            // Check generation is 0
            assert_eq!(va_get_generation(state), 0);

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase3_set_get_cell() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 8, 8, 8);

            // Set some cells alive
            va_set_cell(state, 0, 0, 0, 1);
            va_set_cell(state, 7, 7, 7, 1);
            va_set_cell(state, 3, 4, 5, 1);

            // Verify they are alive
            assert_eq!(va_get_cell(state, 0, 0, 0), 1);
            assert_eq!(va_get_cell(state, 7, 7, 7), 1);
            assert_eq!(va_get_cell(state, 3, 4, 5), 1);

            // Verify others are dead
            assert_eq!(va_get_cell(state, 1, 1, 1), 0);
            assert_eq!(va_get_cell(state, 4, 4, 4), 0);

            // Set a cell to dead
            va_set_cell(state, 0, 0, 0, 0);
            assert_eq!(va_get_cell(state, 0, 0, 0), 0);

            // Test negative coordinates (should be out of bounds)
            va_set_cell(state, -1, 0, 0, 1);
            assert_eq!(va_get_cell(state, -1, 0, 0), 0);

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase3_step_b4s4() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 8, 8, 8);

            // Create a pattern: a cell at (4,4,4) with exactly 4 neighbors
            // This should survive and the dead cells with 4 neighbors should be born
            va_set_cell(state, 4, 4, 4, 1); // Center cell
            va_set_cell(state, 3, 4, 4, 1); // Left
            va_set_cell(state, 5, 4, 4, 1); // Right
            va_set_cell(state, 4, 3, 4, 1); // Front
            va_set_cell(state, 4, 5, 4, 1); // Back

            // Count initial alive cells (should be 5)
            let mut initial_count = 0;
            for z in 0i16..8 {
                for y in 0i16..8 {
                    for x in 0i16..8 {
                        if va_get_cell(state, x, y, z) == 1 {
                            initial_count += 1;
                        }
                    }
                }
            }
            assert_eq!(initial_count, 5);

            // Step the automaton
            va_step(state);

            // Check generation incremented
            assert_eq!(va_get_generation(state), 1);

            // Center cell (4,4,4) had 4 neighbors, should survive
            assert_eq!(va_get_cell(state, 4, 4, 4), 1);

            // Each edge cell had 2 neighbors (center + one other edge), should die
            assert_eq!(va_get_cell(state, 3, 4, 4), 0);
            assert_eq!(va_get_cell(state, 5, 4, 4), 0);
            assert_eq!(va_get_cell(state, 4, 3, 4), 0);
            assert_eq!(va_get_cell(state, 4, 5, 4), 0);

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase3_boundary_conditions() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 4, 4, 4);

            // Test out of bounds access (positive)
            assert_eq!(va_get_cell(state, 10, 10, 10), 0);

            // Test out of bounds access (negative)
            assert_eq!(va_get_cell(state, -1, -1, -1), 0);

            // Set out of bounds (should not crash)
            va_set_cell(state, 10, 10, 10, 1);
            va_set_cell(state, -1, -1, -1, 1);

            // Verify it didn't affect the grid
            let mut count = 0;
            for z in 0i16..4 {
                for y in 0i16..4 {
                    for x in 0i16..4 {
                        if va_get_cell(state, x, y, z) == 1 {
                            count += 1;
                        }
                    }
                }
            }
            assert_eq!(count, 0);

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase4_extract_region_basic() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 8, 8, 8);

            // Set some cells alive
            va_set_cell(state, 2, 2, 2, 1);
            va_set_cell(state, 3, 2, 2, 1);
            va_set_cell(state, 2, 3, 2, 1);

            // Extract a 4x4x4 region containing these cells
            let mut buffer = vec![0u8; 4 * 4 * 4];
            let bytes_written = va_extract_region(
                state,
                buffer.as_mut_ptr(),
                2,
                2,
                2, // min
                6,
                6,
                6, // max
            );

            assert_eq!(bytes_written, 64); // 4*4*4

            // Verify the extracted cells (z,y,x order)
            // Cell (2,2,2) in grid -> (0,0,0) in buffer
            assert_eq!(buffer[0], 1);
            // Cell (3,2,2) in grid -> (0,0,1) in buffer
            assert_eq!(buffer[1], 1);
            // Cell (2,3,2) in grid -> (0,1,0) in buffer (skip 4 cells for y increment)
            assert_eq!(buffer[4], 1);

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase4_extract_region_full_grid() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 4, 4, 4);

            // Set all cells alive
            for z in 0i16..4 {
                for y in 0i16..4 {
                    for x in 0i16..4 {
                        va_set_cell(state, x, y, z, 1);
                    }
                }
            }

            // Extract entire grid
            let mut buffer = vec![0u8; 4 * 4 * 4];
            let bytes_written = va_extract_region(state, buffer.as_mut_ptr(), 0, 0, 0, 4, 4, 4);

            assert_eq!(bytes_written, 64);

            // All cells should be alive
            for &cell in &buffer {
                assert_eq!(cell, 1);
            }

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase4_extract_region_empty() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 4, 4, 4);

            // Extract entire grid (all dead)
            let mut buffer = vec![0u8; 4 * 4 * 4];
            let bytes_written = va_extract_region(state, buffer.as_mut_ptr(), 0, 0, 0, 4, 4, 4);

            assert_eq!(bytes_written, 64);

            // All cells should be dead
            for &cell in &buffer {
                assert_eq!(cell, 0);
            }

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase4_extract_region_out_of_bounds() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 4, 4, 4);

            // Try to extract a region that extends beyond grid bounds
            let mut buffer = vec![0u8; 8 * 8 * 8];
            let bytes_written =
                va_extract_region(state, buffer.as_mut_ptr(), -2, -2, -2, 10, 10, 10);

            // Should be clamped to 4*4*4
            assert_eq!(bytes_written, 64);

            va_destroy(state);
        }
    }

    #[test]
    fn test_phase4_extract_region_null_checks() {
        unsafe {
            let state = va_create();
            va_create_grid(state, 4, 4, 4);
            let mut buffer = vec![0u8; 64];

            // Null state pointer
            assert_eq!(
                va_extract_region(ptr::null(), buffer.as_mut_ptr(), 0, 0, 0, 4, 4, 4),
                0
            );

            // Null buffer pointer
            assert_eq!(
                va_extract_region(state, ptr::null_mut(), 0, 0, 0, 4, 4, 4),
                0
            );

            va_destroy(state);
        }
    }
}
