//! Voxel Automata - 3D Cellular Automata Library
//!
//! This library provides a C ABI for use with LuaJIT FFI in Luanti.

/// Phase 1: FFI Bridge Proof
/// Simple addition function to verify FFI communication works.
#[no_mangle]
pub extern "C" fn va_add(a: i32, b: i32) -> i32 {
    a + b
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
}
