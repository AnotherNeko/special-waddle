//! Core automaton logic and grid operations.
//!
//! This module contains the actual logic for manipulating grid state,
//! stepping the automaton, and extracting/importing regions.
//! The FFI layer in `ffi/` calls these functions.

pub mod field;
pub mod grid;
pub mod region;
pub mod stepping;

pub use field::{
    create_field, field_get, field_in_bounds, field_index_of, field_set, field_step, Field,
};
pub use grid::{count_neighbors, create_grid, in_bounds, index_of};
pub use region::{extract_region, import_region};
pub use stepping::step_automaton;
