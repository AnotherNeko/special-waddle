//! FFI interface for field operations (Phase 6: Integer Field + Delta Diffusion)

use crate::automaton::{create_field, field_get, field_set, field_step, Field};

/// Create a new field with the given dimensions and diffusion rate.
/// Returns a pointer to the allocated Field, or NULL if allocation fails.
#[no_mangle]
pub extern "C" fn va_create_field(
    width: i16,
    height: i16,
    depth: i16,
    diffusion_rate: u8,
) -> *mut Field {
    if width <= 0 || height <= 0 || depth <= 0 {
        return std::ptr::null_mut();
    }

    let field = create_field(width, height, depth, diffusion_rate);
    Box::into_raw(Box::new(field))
}

/// Destroy a field and free its memory.
/// Safe to call with null pointer (no-op).
#[no_mangle]
pub extern "C" fn va_destroy_field(field: *mut Field) {
    if !field.is_null() {
        unsafe {
            let _ = Box::from_raw(field);
        }
    }
}

/// Set a cell value in the field.
/// Out-of-bounds coordinates are silently ignored.
#[no_mangle]
pub extern "C" fn va_field_set(field: *mut Field, x: i16, y: i16, z: i16, value: u32) {
    if field.is_null() {
        return;
    }

    unsafe {
        field_set(&mut *field, x, y, z, value);
    }
}

/// Get a cell value from the field.
/// Get a cell value, returning the non-zero u32 or 0 on error.
/// Returns 0 for out-of-bounds coordinates or null pointer.
#[no_mangle]
pub extern "C" fn va_field_get(field: *const Field, x: i16, y: i16, z: i16) -> u32 {
    if field.is_null() {
        return 0;
    }

    unsafe { field_get(&*field, x, y, z).map(|nz| nz.get()).unwrap_or(0) }
}

/// Step the field forward by one generation using delta-based diffusion.
/// Conservation is guaranteed by construction (Newton's third law for flows).
#[no_mangle]
pub extern "C" fn va_field_step(field: *mut Field) {
    if field.is_null() {
        return;
    }

    unsafe {
        field_step(&mut *field);
    }
}

/// Get the current generation number of the field.
#[no_mangle]
pub extern "C" fn va_field_get_generation(field: *const Field) -> u64 {
    if field.is_null() {
        return 0;
    }

    unsafe { (*field).generation }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_destroy_field() {
        let field = va_create_field(8, 8, 8, 3);
        assert!(!field.is_null());

        unsafe {
            assert_eq!((*field).width, 8);
            assert_eq!((*field).height, 8);
            assert_eq!((*field).depth, 8);
            assert_eq!((*field).generation, 0);
        }

        va_destroy_field(field);
    }

    #[test]
    fn test_field_set_get_via_ffi() {
        let field = va_create_field(8, 8, 8, 3);
        assert!(!field.is_null());

        va_field_set(field, 4, 4, 4, 1000);
        assert_eq!(va_field_get(field, 4, 4, 4), 1000);
        // Unset cells have minimum quantum of 1 (Third Law of thermodynamics)
        assert_eq!(va_field_get(field, 0, 0, 0), 1);

        va_destroy_field(field);
    }

    #[test]
    fn test_field_step_via_ffi() {
        let field = va_create_field(16, 16, 16, 2);
        assert!(!field.is_null());

        va_field_set(field, 8, 8, 8, 1_000_000);

        assert_eq!(va_field_get_generation(field), 0);
        va_field_step(field);
        assert_eq!(va_field_get_generation(field), 1);

        // Value should have spread to neighbors
        assert!(va_field_get(field, 7, 8, 8) > 0);
        assert!(va_field_get(field, 9, 8, 8) > 0);

        va_destroy_field(field);
    }

    #[test]
    fn test_conservation_via_ffi() {
        let field = va_create_field(8, 8, 8, 2);
        assert!(!field.is_null());

        let total_mass = 1_000_000u32;
        va_field_set(field, 4, 4, 4, total_mass);

        let initial_sum: u64 = unsafe { (*field).cells.iter().map(|&v| v as u64).sum() };

        // Step 5 times
        for _ in 0..5 {
            va_field_step(field);
        }

        let final_sum: u64 = unsafe { (*field).cells.iter().map(|&v| v as u64).sum() };

        assert_eq!(initial_sum, final_sum, "Mass not conserved");

        va_destroy_field(field);
    }

    #[test]
    fn test_null_pointer_safety() {
        // These should not crash with null pointers
        va_field_set(std::ptr::null_mut(), 0, 0, 0, 100);
        assert_eq!(va_field_get(std::ptr::null(), 0, 0, 0), 0);
        va_field_step(std::ptr::null_mut());
        assert_eq!(va_field_get_generation(std::ptr::null()), 0);
    }
}
