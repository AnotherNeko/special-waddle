//! Core state structure.
//!
//! This module defines the opaque State type that holds the automaton's grid data.
//! The actual logic for manipulating state is in the `automaton` module.

/// The internal state of a cellular automaton.
///
/// This is an opaque type passed between C and Rust via the FFI layer.
/// All grid manipulation logic should go in the `automaton` module, not here.
pub struct State {
    pub width: i16,
    pub height: i16,
    pub depth: i16,
    pub cells: Vec<u8>, // 0 = dead, 1 = alive
    pub generation: u64,
}
