//! Phase 2: Opaque Handle Lifecycle
//!
//! Handle creation, destruction, and generation counter queries.

use crate::state::State;

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

/// Destroys an automaton state and frees its memory.
/// Safety: ptr must be a valid pointer returned by va_create(), and must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn va_destroy(ptr: *mut State) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

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
