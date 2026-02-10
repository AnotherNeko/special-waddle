//! Voxel Automata - 3D Cellular Automata Library
//!
//! A Rust cdylib + Luanti mod for testing complex voxel-world simulations.
//!
//! ## Module Structure
//!
//! - **`state`**: Core opaque State type (pure data structure)
//! - **`automaton`**: Core simulation logic
//!   - `grid`: Grid operations (index calculation, bounds checking, neighbor counting)
//!   - `stepping`: Cellular automaton stepping with B4/S4 rules
//!   - `region`: Region extraction and import
//! - **`ffi`**: C ABI interface for LuaJIT
//!   - `simple`: va_add (FFI proof of concept)
//!   - `lifecycle`: va_create, va_destroy, va_get_generation
//!   - `grid`: va_create_grid, va_set_cell, va_get_cell, va_step
//!   - `region`: va_extract_region, va_import_region
//!
//! ## Design
//!
//! The library separates concerns:
//! - **Core logic** in `automaton` is tested directly (no FFI overhead)
//! - **FFI layer** is minimal, just wrapping core logic with null checks and pointer safety
//! - **Tests** are co-located with their implementations for clarity

pub mod automaton;
pub mod ffi;
pub mod state;

// Re-export public FFI API for C bindings
pub use automaton::Field;
pub use ffi::{
    va_add, va_create, va_create_field, va_create_grid, va_destroy, va_extract_region,
    va_field_get, va_field_set, va_field_step, va_get_cell, va_get_generation, va_import_region,
    va_set_cell, va_step,
};
pub use state::State;
