//! Phase 1: FFI Bridge Proof
//!
//! Simple addition function to verify FFI communication works.

/// Simple addition function to verify FFI communication works.
#[no_mangle]
pub extern "C" fn va_add(a: i32, b: i32) -> i32 {
    a + b
}
