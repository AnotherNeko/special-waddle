//! Voxel Automata - 3D Cellular Automata Library
//!
//! This library provides a C ABI for use with LuaJIT FFI in Luanti.

/// The internal state of a cellular automaton
pub struct State {
    generation: u64,
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
    let state = Box::new(State { generation: 0 });
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
}
