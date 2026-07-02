//! C FFI layer for Luanti integration.
//!
//! This module exports C ABI functions for use with LuaJIT FFI.
//! All functions are marked with `#[no_mangle]` and use `extern "C"`.
//!
//! The actual logic is in the `automaton` module. These functions are thin wrappers
//! that handle null checks, pointer safety, and C-to-Rust conversions.

pub mod cadence;
pub mod field;
pub mod grid;
pub mod incremental;
pub mod lifecycle;
pub mod region;
pub mod simple;

pub use cadence::{
    va_sc_cadence_advance, va_sc_cadence_bisect, va_sc_cadence_lookup, va_sc_cadence_merge_poll,
    va_sc_cadence_step, va_sc_global_tick, va_sc_infinity_create, va_sc_infinity_destroy,
};
pub use field::{
    va_create_field, va_destroy_field, va_field_get, va_field_get_generation, va_field_set,
    va_field_step,
};
pub use grid::{va_create_grid, va_get_cell, va_set_cell, va_step};
pub use incremental::{
    va_create_step_controller, va_destroy_step_controller, va_sc_begin_step, va_sc_field_get,
    va_sc_field_get_generation, va_sc_field_set, va_sc_is_stepping, va_sc_step_blocking,
    va_sc_tick,
};
pub use lifecycle::{va_create, va_destroy, va_get_generation};
pub use region::{va_extract_region, va_import_region};
pub use simple::va_add;
