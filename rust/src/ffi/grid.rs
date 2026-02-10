//! Grid creation, cell access, and stepping.

use crate::automaton;
use crate::state::State;

/// Creates a grid with the specified dimensions.
///
/// # Safety
/// - `ptr` must be a valid pointer to a State
///
/// # Returns
/// 0 on success, 1 on failure (null pointer)
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
    automaton::create_grid(state, width, height, depth);
    0
}

/// Sets a cell to alive (1) or dead (0).
///
/// # Safety
/// - `ptr` must be a valid pointer to a State with a grid
///
/// Out-of-bounds coordinates are silently ignored.
#[no_mangle]
pub unsafe extern "C" fn va_set_cell(ptr: *mut State, x: i16, y: i16, z: i16, alive: u8) {
    if ptr.is_null() {
        return;
    }

    let state = &mut *ptr;
    if !automaton::grid::in_bounds(state, x, y, z) {
        return;
    }

    let idx = automaton::grid::index_of(state, x, y, z);
    state.cells[idx] = if alive != 0 { 1 } else { 0 };
}

/// Gets the state of a cell (0 = dead, 1 = alive).
///
/// # Safety
/// - `ptr` must be a valid pointer to a State with a grid
///
/// # Returns
/// 0 if out of bounds, null pointer, or dead; 1 if alive.
#[no_mangle]
pub unsafe extern "C" fn va_get_cell(ptr: *const State, x: i16, y: i16, z: i16) -> u8 {
    if ptr.is_null() {
        return 0;
    }

    let state = &*ptr;
    if !automaton::grid::in_bounds(state, x, y, z) {
        return 0;
    }

    let idx = automaton::grid::index_of(state, x, y, z);
    state.cells[idx]
}

/// Advances the cellular automaton by one generation.
///
/// # Safety
/// - `ptr` must be a valid pointer to a State with a grid
///
/// Uses B4/S4 rules with Moore neighborhood (26 neighbors).
#[no_mangle]
pub unsafe extern "C" fn va_step(ptr: *mut State) {
    if ptr.is_null() {
        return;
    }

    let state = &mut *ptr;
    automaton::step_automaton(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::lifecycle;
    use std::ptr;

    #[test]
    fn test_create_grid() {
        unsafe {
            let state = lifecycle::va_create();
            assert!(!state.is_null());

            let result = va_create_grid(state, 8, 8, 8);
            assert_eq!(result, 0);

            // Verify all cells are dead
            for z in 0i16..8 {
                for y in 0i16..8 {
                    for x in 0i16..8 {
                        assert_eq!(va_get_cell(state, x, y, z), 0);
                    }
                }
            }

            lifecycle::va_destroy(state);
        }
    }

    #[test]
    fn test_set_and_get_cell() {
        unsafe {
            let state = lifecycle::va_create();
            va_create_grid(state, 8, 8, 8);

            va_set_cell(state, 0, 0, 0, 1);
            assert_eq!(va_get_cell(state, 0, 0, 0), 1);

            va_set_cell(state, 0, 0, 0, 0);
            assert_eq!(va_get_cell(state, 0, 0, 0), 0);

            lifecycle::va_destroy(state);
        }
    }

    #[test]
    fn test_out_of_bounds_access() {
        unsafe {
            let state = lifecycle::va_create();
            va_create_grid(state, 4, 4, 4);

            assert_eq!(va_get_cell(state, -1, 0, 0), 0);
            assert_eq!(va_get_cell(state, 4, 0, 0), 0);

            // Should not crash
            va_set_cell(state, -1, 0, 0, 1);
            va_set_cell(state, 4, 0, 0, 1);

            lifecycle::va_destroy(state);
        }
    }

    #[test]
    fn test_step() {
        unsafe {
            let state = lifecycle::va_create();
            va_create_grid(state, 8, 8, 8);

            // Set up cross pattern
            va_set_cell(state, 4, 4, 4, 1);
            va_set_cell(state, 3, 4, 4, 1);
            va_set_cell(state, 5, 4, 4, 1);
            va_set_cell(state, 4, 3, 4, 1);
            va_set_cell(state, 4, 5, 4, 1);

            assert_eq!(lifecycle::va_get_generation(state), 0);

            va_step(state);

            assert_eq!(lifecycle::va_get_generation(state), 1);
            // Center survives (4 neighbors)
            assert_eq!(va_get_cell(state, 4, 4, 4), 1);
            // Edges die (2 neighbors)
            assert_eq!(va_get_cell(state, 3, 4, 4), 0);

            lifecycle::va_destroy(state);
        }
    }

    #[test]
    fn test_null_pointer_handling() {
        unsafe {
            assert_eq!(va_create_grid(ptr::null_mut(), 8, 8, 8), 1);
            va_set_cell(ptr::null_mut(), 0, 0, 0, 1); // Should not crash
            assert_eq!(va_get_cell(ptr::null(), 0, 0, 0), 0);
            va_step(ptr::null_mut()); // Should not crash
        }
    }
}
