//! Region extraction and import FFI functions.

use crate::automaton;
use crate::state::State;

/// Extracts a rectangular region of cells into a flat output buffer.
///
/// # Layout
/// The buffer is filled in z,y,x order (z changes slowest, x changes fastest).
/// This matches the layout expected by `va_import_region`.
///
/// # Safety
/// - `ptr` must be a valid pointer to a State with a grid, or null
/// - `out_buf` must point to a buffer with at least
///   `(max_x - min_x) * (max_y - min_y) * (max_z - min_z)` bytes
///
/// # Returns
/// Number of bytes written, or 0 on error.
#[no_mangle]
pub unsafe extern "C" fn va_extract_region(
    ptr: *const State,
    out_buf: *mut u8,
    min_x: i16,
    min_y: i16,
    min_z: i16,
    max_x: i16,
    max_y: i16,
    max_z: i16,
) -> u64 {
    if ptr.is_null() || out_buf.is_null() {
        return 0;
    }

    let state = &*ptr;
    if state.cells.is_empty() {
        return 0;
    }

    let width = ((max_x - min_x).max(0)) as usize;
    let height = ((max_y - min_y).max(0)) as usize;
    let depth = ((max_z - min_z).max(0)) as usize;

    let buf_slice = std::slice::from_raw_parts_mut(out_buf, width * height * depth);
    automaton::extract_region(state, buf_slice, min_x, min_y, min_z, max_x, max_y, max_z)
}

/// Imports a rectangular region of cells from a flat buffer.
///
/// # Layout
/// The buffer is expected to be in z,y,x order (matching `va_extract_region`).
/// Input values are normalized: 0 = dead, non-zero = alive.
///
/// # Safety
/// - `ptr` must be a valid pointer to a State with a grid, or null
/// - `in_buf` must point to a buffer with at least
///   `(max_x - min_x) * (max_y - min_y) * (max_z - min_z)` bytes
///
/// # Returns
/// Number of bytes read, or 0 on error.
#[no_mangle]
pub unsafe extern "C" fn va_import_region(
    ptr: *mut State,
    in_buf: *const u8,
    min_x: i16,
    min_y: i16,
    min_z: i16,
    max_x: i16,
    max_y: i16,
    max_z: i16,
) -> u64 {
    if ptr.is_null() || in_buf.is_null() {
        return 0;
    }

    let state = &mut *ptr;

    let width = ((max_x - min_x).max(0)) as usize;
    let height = ((max_y - min_y).max(0)) as usize;
    let depth = ((max_z - min_z).max(0)) as usize;

    let buf_slice = std::slice::from_raw_parts(in_buf, width * height * depth);
    automaton::import_region(state, buf_slice, min_x, min_y, min_z, max_x, max_y, max_z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_extract_region() {
        unsafe {
            let state = crate::ffi::lifecycle::va_create();
            crate::ffi::grid::va_create_grid(state, 8, 8, 8);

            crate::ffi::grid::va_set_cell(state, 2, 2, 2, 1);
            crate::ffi::grid::va_set_cell(state, 3, 2, 2, 1);

            let mut buffer = vec![0u8; 64];
            let bytes = va_extract_region(state, buffer.as_mut_ptr(), 2, 2, 2, 6, 6, 6);

            assert_eq!(bytes, 64);
            assert_eq!(buffer[0], 1);
            assert_eq!(buffer[1], 1);

            crate::ffi::lifecycle::va_destroy(state);
        }
    }

    #[test]
    fn test_import_region() {
        unsafe {
            let state = crate::ffi::lifecycle::va_create();
            crate::ffi::grid::va_create_grid(state, 8, 8, 8);

            let buffer = vec![
                1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0,
            ];

            let bytes = va_import_region(state, buffer.as_ptr(), 2, 2, 2, 6, 6, 6);

            assert_eq!(bytes, 64);
            assert_eq!(crate::ffi::grid::va_get_cell(state, 2, 2, 2), 1);
            assert_eq!(crate::ffi::grid::va_get_cell(state, 3, 2, 2), 1);

            crate::ffi::lifecycle::va_destroy(state);
        }
    }

    #[test]
    fn test_null_pointer_handling() {
        unsafe {
            let mut buffer = vec![0u8; 64];

            assert_eq!(
                va_extract_region(ptr::null(), buffer.as_mut_ptr(), 0, 0, 0, 4, 4, 4),
                0
            );
            assert_eq!(
                va_extract_region(
                    ptr::null_mut() as *const State,
                    ptr::null_mut(),
                    0,
                    0,
                    0,
                    4,
                    4,
                    4
                ),
                0
            );

            assert_eq!(
                va_import_region(ptr::null_mut(), buffer.as_ptr(), 0, 0, 0, 4, 4, 4),
                0
            );
            assert_eq!(
                va_import_region(ptr::null_mut(), ptr::null(), 0, 0, 0, 4, 4, 4),
                0
            );
        }
    }
}
