//! Voxel Automata - 3D Cellular Automata Library
//!
//! This library provides a C ABI for use with LuaJIT FFI in Luanti.

/// The internal state of a cellular automaton
pub struct State {
    width: i16,
    height: i16,
    depth: i16,
    cells: Vec<u8>, // 0 = dead, 1 = alive
    generation: u64,
}

impl State {
    /// Get the linear index for a 3D coordinate
    #[inline]
    fn index(&self, x: i16, y: i16, z: i16) -> usize {
        z as usize * self.height as usize * self.width as usize
            + y as usize * self.width as usize
            + x as usize
    }

    /// Count alive neighbors using Moore neighborhood (26 neighbors)
    fn count_neighbors(&self, x: i16, y: i16, z: i16) -> u8 {
        let mut count = 0;

        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    // Skip the center cell
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }

                    let nx = x + dx;
                    let ny = y + dy;
                    let nz = z + dz;

                    // Check bounds
                    if nx >= 0
                        && nx < self.width
                        && ny >= 0
                        && ny < self.height
                        && nz >= 0
                        && nz < self.depth
                    {
                        let idx = self.index(nx, ny, nz);
                        count += self.cells[idx];
                    }
                }
            }
        }

        count
    }
}

/// Phase 1: FFI Bridge Proof
/// Simple addition function to verify FFI communication works.
#[no_mangle]
pub extern "C" fn va_add(a: i32, b: i32) -> i32 {
    a + b
}

/// Phase 2: Opaque Handle Lifecycle
/// Creates a new automaton state and returns an opaque pointer.
/// Returns null on allocation failure.
#[no_mangle]
pub extern "C" fn va_create() -> *mut State {
    let state = Box::new(State {
        width: 0,
        height: 0,
        depth: 0,
        cells: Vec::new(),
        generation: 0,
    });
    Box::into_raw(state)
}

/// Phase 2: Opaque Handle Lifecycle
/// Destroys an automaton state and frees its memory.
/// Safety: ptr must be a valid pointer returned by va_create(), and must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn va_destroy(ptr: *mut State) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

/// Phase 2: Opaque Handle Lifecycle
/// Gets the current generation counter from a state.
/// Safety: ptr must be a valid pointer to a State.
/// Returns 0 if ptr is null.
#[no_mangle]
pub unsafe extern "C" fn va_get_generation(ptr: *const State) -> u64 {
    if ptr.is_null() {
        return 0;
    }
    (*ptr).generation
}

/// Phase 3: Small Grid + Step
/// Creates a grid with the specified dimensions.
/// Safety: ptr must be a valid pointer to a State.
/// Returns 0 on success, 1 on failure (null pointer or allocation failure).
#[no_mangle]
pub unsafe extern "C" fn va_create_grid(
    ptr: *mut State,
    width: i16,
    height: i16,
    depth: i16,
) -> i32 {
    if ptr.is_null() {
        return 1;
    }

    let state = &mut *ptr;
    let size = (width as usize) * (height as usize) * (depth as usize);

    state.width = width;
    state.height = height;
    state.depth = depth;
    state.cells = vec![0; size];
    state.generation = 0;

    0
}

/// Phase 3: Small Grid + Step
/// Sets a cell to alive (1) or dead (0).
/// Safety: ptr must be a valid pointer to a State with a grid.
/// Coordinates must be within bounds.
#[no_mangle]
pub unsafe extern "C" fn va_set_cell(ptr: *mut State, x: i16, y: i16, z: i16, alive: u8) {
    if ptr.is_null() {
        return;
    }

    let state = &mut *ptr;
    if x < 0 || x >= state.width || y < 0 || y >= state.height || z < 0 || z >= state.depth {
        return;
    }

    let idx = state.index(x, y, z);
    state.cells[idx] = if alive != 0 { 1 } else { 0 };
}

/// Phase 3: Small Grid + Step
/// Gets the state of a cell (0 = dead, 1 = alive).
/// Safety: ptr must be a valid pointer to a State with a grid.
/// Returns 0 if out of bounds or null pointer.
#[no_mangle]
pub unsafe extern "C" fn va_get_cell(ptr: *const State, x: i16, y: i16, z: i16) -> u8 {
    if ptr.is_null() {
        return 0;
    }

    let state = &*ptr;
    if x < 0 || x >= state.width || y < 0 || y >= state.height || z < 0 || z >= state.depth {
        return 0;
    }

    let idx = state.index(x, y, z);
    state.cells[idx]
}

/// Phase 3: Small Grid + Step
/// Advances the cellular automaton by one generation using B4/S4 rules (Moore neighborhood).
/// Birth on 4 neighbors, survival on 4 neighbors.
/// Safety: ptr must be a valid pointer to a State with a grid.
#[no_mangle]
pub unsafe extern "C" fn va_step(ptr: *mut State) {
    if ptr.is_null() {
        return;
    }

    let state = &mut *ptr;
    if state.cells.is_empty() {
        return;
    }

    let mut next_cells = vec![0; state.cells.len()];

    for z in 0..state.depth as u32 {
        for y in 0..state.height as u32 {
            for x in 0..state.width as u32 {
                let neighbors = state.count_neighbors(x as i16, y as i16, z as i16);
                let idx = state.index(x as i16, y as i16, z as i16);
                let current = state.cells[idx];

                // B4/S4 rule: Birth on 4, Survival on 4
                next_cells[idx] = if current == 1 {
                    // Alive cell
                    if neighbors == 4 {
                        1
                    } else {
                        0
                    }
                } else {
                    // Dead cell
                    if neighbors == 4 {
                        1
                    } else {
                        0
                    }
                };
            }
        }
    }

    state.cells = next_cells;
    state.generation += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_va_add() {
        assert_eq!(va_add(2, 3), 5);
        assert_eq!(va_add(-1, 1), 0);
        assert_eq!(va_add(0, 0), 0);
    }

    #[test]
    fn test_phase2_lifecycle() {
        use std::ptr;
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
}
