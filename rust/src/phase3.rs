//! Phase 3: Small Grid + Step
//!
//! Grid creation, cell manipulation, and cellular automaton stepping with B4/S4 rules.

use crate::state::State;

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
