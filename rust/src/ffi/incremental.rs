//! FFI interface for incremental stepping (Phase 8: Non-Blocking Incremental Stepping)

use crate::automaton::incremental::StepController;

/// Create a new StepController with the given dimensions and thread pool size.
/// Returns a pointer to the allocated StepController, or NULL if allocation fails.
#[no_mangle]
pub extern "C" fn va_create_step_controller(
    width: i16,
    height: i16,
    depth: i16,
    diffusion_rate: u8,
    num_threads: u8,
) -> *mut StepController {
    if width <= 0 || height <= 0 || depth <= 0 {
        return std::ptr::null_mut();
    }

    let ctrl = StepController::new(width, height, depth, diffusion_rate, num_threads);
    Box::into_raw(Box::new(ctrl))
}

/// Destroy a StepController and free its memory.
/// Safe to call with null pointer (no-op).
#[no_mangle]
pub extern "C" fn va_destroy_step_controller(ctrl: *mut StepController) {
    if !ctrl.is_null() {
        unsafe {
            let _ = Box::from_raw(ctrl);
        }
    }
}

/// Set a cell value in the inner field.
/// Out-of-bounds coordinates are silently ignored.
/// Returns early if a step is currently active (prevent mid-step mutation).
#[no_mangle]
pub extern "C" fn va_sc_field_set(ctrl: *mut StepController, x: i16, y: i16, z: i16, value: u32) {
    if ctrl.is_null() {
        return;
    }

    unsafe {
        let ctrl = &mut *ctrl;
        if ctrl.is_stepping() {
            return; // Prevent mutation during active step
        }
        crate::automaton::field_set(&mut ctrl.field, x, y, z, value);
    }
}

/// Get a cell value from the inner field.
/// Get a cell value, returning the non-zero u32 or 0 on error.
/// Returns 0 for out-of-bounds coordinates or null pointer.
#[no_mangle]
pub extern "C" fn va_sc_field_get(ctrl: *const StepController, x: i16, y: i16, z: i16) -> u32 {
    if ctrl.is_null() {
        return 0;
    }

    unsafe {
        crate::automaton::field_get(&(*ctrl).field, x, y, z)
            .map(|nz| nz.get())
            .unwrap_or(0)
    }
}

/// Get the current generation number of the inner field.
#[no_mangle]
pub extern "C" fn va_sc_field_get_generation(ctrl: *const StepController) -> u64 {
    if ctrl.is_null() {
        return 0;
    }

    unsafe { (*ctrl).field.generation }
}

/// Begin a new incremental step.
/// Returns 0 on success, 1 if a step is already in progress.
#[no_mangle]
pub extern "C" fn va_sc_begin_step(ctrl: *mut StepController) -> i32 {
    if ctrl.is_null() {
        return -1;
    }

    unsafe {
        match (*ctrl).begin_step() {
            Ok(()) => 0,
            Err(()) => 1,
        }
    }
}

/// Do bounded work within the given time budget (microseconds).
/// Returns 1 if the step completed during this tick, 0 if more work remains, -1 if no step is active.
#[no_mangle]
pub extern "C" fn va_sc_tick(ctrl: *mut StepController, budget_us: u64) -> i32 {
    if ctrl.is_null() {
        return -1;
    }

    unsafe {
        let ctrl = &mut *ctrl;
        if !ctrl.is_stepping() {
            return -1;
        }
        if ctrl.tick(budget_us) {
            1
        } else {
            0
        }
    }
}

/// Query whether a step is currently in progress.
/// Returns 1 if stepping, 0 if idle, -1 if null pointer.
#[no_mangle]
pub extern "C" fn va_sc_is_stepping(ctrl: *const StepController) -> i32 {
    if ctrl.is_null() {
        return -1;
    }

    unsafe {
        if (*ctrl).is_stepping() {
            1
        } else {
            0
        }
    }
}

/// Convenience: blocking full step (equivalent to begin_step + tick(MAX) until done).
#[no_mangle]
pub extern "C" fn va_sc_step_blocking(ctrl: *mut StepController) {
    if ctrl.is_null() {
        return;
    }

    unsafe {
        (*ctrl).step_blocking();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_destroy_step_controller() {
        let ctrl = va_create_step_controller(16, 16, 16, 2, 1);
        assert!(!ctrl.is_null());

        unsafe {
            assert_eq!((*ctrl).field.width, 16);
            assert_eq!((*ctrl).field.height, 16);
            assert_eq!((*ctrl).field.depth, 16);
        }

        va_destroy_step_controller(ctrl);
    }

    #[test]
    fn test_field_set_get_via_ffi() {
        let ctrl = va_create_step_controller(16, 16, 16, 2, 1);
        assert!(!ctrl.is_null());

        va_sc_field_set(ctrl, 8, 8, 8, 5000);
        assert_eq!(va_sc_field_get(ctrl, 8, 8, 8), 5000);
        // Unset cells have minimum quantum of 1 (Third Law of thermodynamics)
        assert_eq!(va_sc_field_get(ctrl, 0, 0, 0), 1);

        va_destroy_step_controller(ctrl);
    }

    #[test]
    fn test_step_blocking_via_ffi() {
        let ctrl = va_create_step_controller(16, 16, 16, 2, 1);
        assert!(!ctrl.is_null());

        va_sc_field_set(ctrl, 8, 8, 8, 1_000_000);

        assert_eq!(va_sc_field_get_generation(ctrl), 0);
        va_sc_step_blocking(ctrl);
        assert_eq!(va_sc_field_get_generation(ctrl), 1);

        // Value should have spread to neighbors
        assert!(va_sc_field_get(ctrl, 7, 8, 8) > 0);
        assert!(va_sc_field_get(ctrl, 9, 8, 8) > 0);

        va_destroy_step_controller(ctrl);
    }

    #[test]
    fn test_begin_step_and_tick() {
        let ctrl = va_create_step_controller(16, 16, 16, 2, 1);
        assert!(!ctrl.is_null());

        va_sc_field_set(ctrl, 8, 8, 8, 500_000);

        assert_eq!(va_sc_is_stepping(ctrl), 0); // Not stepping initially
        assert_eq!(va_sc_begin_step(ctrl), 0); // Success
        assert_eq!(va_sc_is_stepping(ctrl), 1); // Now stepping

        // Second begin should fail
        assert_eq!(va_sc_begin_step(ctrl), 1); // Already stepping

        // Tick until done (4 MB budget is plenty for 16^3)
        let mut done = false;
        for _ in 0..100 {
            let result = va_sc_tick(ctrl, 4_000_000);
            if result == 1 {
                done = true;
                break;
            }
            assert!(result == 0, "Unexpected error from tick");
        }

        assert!(done, "Step should complete within 100 ticks");
        assert_eq!(va_sc_is_stepping(ctrl), 0); // Done stepping
        assert_eq!(va_sc_field_get_generation(ctrl), 1);

        va_destroy_step_controller(ctrl);
    }

    #[test]
    fn test_conservation_via_ffi() {
        let ctrl = va_create_step_controller(16, 16, 16, 2, 1);
        assert!(!ctrl.is_null());

        va_sc_field_set(ctrl, 8, 8, 8, 1_000_000);

        let initial_sum: u64 = unsafe { (*ctrl).field.cells.iter().map(|&v| v as u64).sum() };

        // Step 3 times
        for _ in 0..3 {
            va_sc_step_blocking(ctrl);
        }

        let final_sum: u64 = unsafe { (*ctrl).field.cells.iter().map(|&v| v as u64).sum() };

        assert_eq!(initial_sum, final_sum, "Mass not conserved via FFI");

        va_destroy_step_controller(ctrl);
    }

    #[test]
    fn test_null_pointer_safety() {
        // These should not crash with null pointers
        va_sc_field_set(std::ptr::null_mut(), 0, 0, 0, 100);
        assert_eq!(va_sc_field_get(std::ptr::null(), 0, 0, 0), 0);
        assert_eq!(va_sc_field_get_generation(std::ptr::null()), 0);
        assert_eq!(va_sc_begin_step(std::ptr::null_mut()), -1);
        assert_eq!(va_sc_tick(std::ptr::null_mut(), 4000), -1);
        assert_eq!(va_sc_is_stepping(std::ptr::null()), -1);
        va_sc_step_blocking(std::ptr::null_mut());
        va_destroy_step_controller(std::ptr::null_mut());
    }

    #[test]
    fn test_mutation_blocked_during_step() {
        let ctrl = va_create_step_controller(16, 16, 16, 2, 1);
        assert!(!ctrl.is_null());

        va_sc_field_set(ctrl, 8, 8, 8, 500_000);

        va_sc_begin_step(ctrl);
        assert_eq!(va_sc_is_stepping(ctrl), 1);

        // Try to set a cell while stepping — should be ignored
        let before = va_sc_field_get(ctrl, 0, 0, 0);
        va_sc_field_set(ctrl, 0, 0, 0, 999_999);
        let after = va_sc_field_get(ctrl, 0, 0, 0);

        assert_eq!(
            before, after,
            "Field mutation should be blocked during step"
        );

        // Finish the step
        while va_sc_tick(ctrl, 4_000_000) == 0 {}

        // Now mutation should work
        va_sc_field_set(ctrl, 0, 0, 0, 777_777);
        assert_eq!(va_sc_field_get(ctrl, 0, 0, 0), 777_777);

        va_destroy_step_controller(ctrl);
    }
}
