//! State creation, destruction, and generation queries.

use crate::state::State;

/// Creates a new automaton state and returns an opaque pointer.
///
/// # Returns
/// A pointer to a new State, or null on allocation failure.
///
/// # Safety
/// The returned pointer must eventually be freed with `va_destroy()`.
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

/// Destroys an automaton state and frees its memory.
///
/// # Safety
/// - `ptr` must be a valid pointer returned by `va_create()`, or null
/// - `ptr` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn va_destroy(ptr: *mut State) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

/// Gets the current generation counter from a state.
///
/// # Safety
/// - `ptr` must be a valid pointer to a State, or null
///
/// # Returns
/// The generation counter, or 0 if ptr is null.
#[no_mangle]
pub unsafe extern "C" fn va_get_generation(ptr: *const State) -> u64 {
    if ptr.is_null() {
        return 0;
    }
    (*ptr).generation
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_create_and_destroy() {
        unsafe {
            let state = va_create();
            assert!(!state.is_null());

            // Should not crash
            va_destroy(state);
        }
    }

    #[test]
    fn test_initial_generation() {
        unsafe {
            let state = va_create();
            assert_eq!(va_get_generation(state), 0);
            va_destroy(state);
        }
    }

    #[test]
    fn test_destroy_null() {
        unsafe {
            // Should not crash
            va_destroy(ptr::null_mut());
        }
    }

    #[test]
    fn test_get_generation_null() {
        unsafe {
            assert_eq!(va_get_generation(ptr::null()), 0);
        }
    }
}
