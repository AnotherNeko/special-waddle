//! Voxel Automata - 3D Cellular Automata Library
//!
//! This library provides a C ABI for use with LuaJIT FFI in Luanti.
//!
//! ## Module Structure
//!
//! - `state`: Core state structure and helper methods
//! - `phase1`: FFI bridge proof (simple addition)
//! - `phase2`: Opaque handle lifecycle (create, destroy, generation)
//! - `phase3`: Grid and stepping (B4/S4 cellular automaton)
//! - `phase4`: Visualization (region extraction)

pub mod state;

mod phase1;
mod phase2;
mod phase3;
mod phase4;

#[cfg(test)]
mod tests;

// Re-export public FFI API
pub use phase1::va_add;
pub use phase2::{va_create, va_destroy, va_get_generation};
pub use phase3::{va_create_grid, va_get_cell, va_set_cell, va_step};
pub use phase4::va_extract_region;
pub use state::State;
