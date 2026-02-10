//! Core automaton logic and grid operations.
//!
//! This module contains the actual logic for manipulating grid state,
//! stepping the automaton, and extracting/importing regions.
//! The FFI layer in `ffi/` calls these functions.

pub mod grid;
pub mod region;
pub mod stepping;

pub use grid::{count_neighbors, create_grid, in_bounds, index_of};
pub use region::{extract_region, import_region};
pub use stepping::step_automaton;
