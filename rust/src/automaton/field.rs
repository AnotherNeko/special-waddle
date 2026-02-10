//! Integer field with delta-based diffusion.
//!
//! Implements conservation-safe diffusion on integer grids:
//! - Process each axis independently (X, Y, Z)
//! - For each adjacent cell pair, compute flow = (cell_a - cell_b) / divisor
//! - Apply symmetrically: cell_a -= flow, cell_b += flow
//! - This ensures conservation by Newton's third law
//! - Copy result back to field before next axis (prevents over-application)

/// A 3D field of u32 values.
/// Used for dense simulations like weather, thermal diffusion, or chemistry.
pub struct Field {
    pub width: i16,
    pub height: i16,
    pub depth: i16,
    pub cells: Vec<u32>, // u32 per cell (e.g. centigrams, microkelvin)
    pub generation: u64,
    pub diffusion_rate: u8, // power-of-2 shift (e.g. 3 = divide by 8)
}

/// Initialize a field with the given dimensions and diffusion rate.
pub fn create_field(width: i16, height: i16, depth: i16, diffusion_rate: u8) -> Field {
    let size = (width as usize) * (height as usize) * (depth as usize);
    Field {
        width,
        height,
        depth,
        cells: vec![0; size],
        generation: 0,
        diffusion_rate,
    }
}

/// Calculate the linear index for a 3D coordinate.
#[inline]
pub fn field_index_of(field: &Field, x: i16, y: i16, z: i16) -> usize {
    z as usize * field.height as usize * field.width as usize
        + y as usize * field.width as usize
        + x as usize
}

/// Check if coordinates are within field bounds.
#[inline]
pub fn field_in_bounds(field: &Field, x: i16, y: i16, z: i16) -> bool {
    x >= 0 && x < field.width && y >= 0 && y < field.height && z >= 0 && z < field.depth
}

/// Set a cell value.
pub fn field_set(field: &mut Field, x: i16, y: i16, z: i16, value: u32) {
    if field_in_bounds(field, x, y, z) {
        let idx = field_index_of(field, x, y, z);
        field.cells[idx] = value;
    }
}

/// Get a cell value.
pub fn field_get(field: &Field, x: i16, y: i16, z: i16) -> u32 {
    if field_in_bounds(field, x, y, z) {
        let idx = field_index_of(field, x, y, z);
        field.cells[idx]
    } else {
        0
    }
}

/// Step the field forward by one generation using axis-aligned diffusion.
/// Processes each axis (X, Y, Z) independently, computing and applying flows inline.
/// Between axes, copies results back to preserve conservation.
pub fn field_step(field: &mut Field) {
    let rate = field.diffusion_rate;
    let divisor = 1u32 << rate; // 2^rate

    let mut new_cells = field.cells.clone();

    // X-axis diffusion: each pair (x, x+1) exchanges
    for z in 0..field.depth {
        for y in 0..field.height {
            for x in 0..field.width - 1 {
                let idx_a = field_index_of(field, x, y, z);
                let idx_b = field_index_of(field, x + 1, y, z);

                let cell_a = field.cells[idx_a] as i64;
                let cell_b = field.cells[idx_b] as i64;
                let flow = (cell_a - cell_b) / (divisor as i64);

                new_cells[idx_a] = ((new_cells[idx_a] as i64) - flow).max(0) as u32;
                new_cells[idx_b] = ((new_cells[idx_b] as i64) + flow).max(0) as u32;
            }
        }
    }

    // Copy result back before next axis
    for i in 0..field.cells.len() {
        field.cells[i] = new_cells[i];
    }

    // Y-axis diffusion: each pair (y, y+1) exchanges
    for z in 0..field.depth {
        for y in 0..field.height - 1 {
            for x in 0..field.width {
                let idx_a = field_index_of(field, x, y, z);
                let idx_b = field_index_of(field, x, y + 1, z);

                let cell_a = field.cells[idx_a] as i64;
                let cell_b = field.cells[idx_b] as i64;
                let flow = (cell_a - cell_b) / (divisor as i64);

                new_cells[idx_a] = ((new_cells[idx_a] as i64) - flow).max(0) as u32;
                new_cells[idx_b] = ((new_cells[idx_b] as i64) + flow).max(0) as u32;
            }
        }
    }

    // Copy result back before next axis
    for i in 0..field.cells.len() {
        field.cells[i] = new_cells[i];
    }

    // Z-axis diffusion: each pair (z, z+1) exchanges
    for z in 0..field.depth - 1 {
        for y in 0..field.height {
            for x in 0..field.width {
                let idx_a = field_index_of(field, x, y, z);
                let idx_b = field_index_of(field, x, y, z + 1);

                let cell_a = field.cells[idx_a] as i64;
                let cell_b = field.cells[idx_b] as i64;
                let flow = (cell_a - cell_b) / (divisor as i64);

                new_cells[idx_a] = ((new_cells[idx_a] as i64) - flow).max(0) as u32;
                new_cells[idx_b] = ((new_cells[idx_b] as i64) + flow).max(0) as u32;
            }
        }
    }

    field.cells = new_cells;
    field.generation += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_field() {
        let field = create_field(8, 8, 8, 3);
        assert_eq!(field.width, 8);
        assert_eq!(field.height, 8);
        assert_eq!(field.depth, 8);
        assert_eq!(field.cells.len(), 512);
        assert_eq!(field.generation, 0);
        assert_eq!(field.diffusion_rate, 3);
        assert!(field.cells.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_field_set_get() {
        let mut field = create_field(8, 8, 8, 3);

        field_set(&mut field, 4, 4, 4, 1000);
        assert_eq!(field_get(&field, 4, 4, 4), 1000);
        assert_eq!(field_get(&field, 0, 0, 0), 0);

        // Out of bounds reads return 0
        assert_eq!(field_get(&field, -1, 0, 0), 0);
        assert_eq!(field_get(&field, 8, 0, 0), 0);
    }

    #[test]
    fn test_conservation_single_cell() {
        // Test that the total mass (sum of all cells) is preserved after stepping
        let mut field = create_field(8, 8, 8, 2);

        let total_mass = 1_000_000u32;
        field_set(&mut field, 4, 4, 4, total_mass);

        let initial_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        // Step multiple times
        for _ in 0..10 {
            field_step(&mut field);
        }

        let final_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        // Should be exactly equal (conservation by construction)
        assert_eq!(
            initial_sum, final_sum,
            "Mass not conserved: {} != {}",
            initial_sum, final_sum
        );
    }

    #[test]
    fn test_diffusion_spreads_symmetric() {
        // Test that diffusion spreads symmetrically from a point source
        let mut field = create_field(16, 16, 16, 2);

        let center_val = 1_000_000u32;
        field_set(&mut field, 8, 8, 8, center_val);

        field_step(&mut field);

        // Check that neighbors got some value
        let neighbors_have_value = field_get(&field, 7, 8, 8) > 0
            && field_get(&field, 9, 8, 8) > 0
            && field_get(&field, 8, 7, 8) > 0
            && field_get(&field, 8, 9, 8) > 0
            && field_get(&field, 8, 8, 7) > 0
            && field_get(&field, 8, 8, 9) > 0;

        assert!(
            neighbors_have_value,
            "Neighbors should have non-zero values"
        );

        // Check total is still conserved
        let total: u64 = field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(total, center_val as u64, "Total mass should be conserved");
    }

    #[test]
    fn test_diffusion_spreads_from_edge() {
        // Test spreading from a cell at the edge (boundary condition)
        let mut field = create_field(8, 8, 8, 2);

        field_set(&mut field, 0, 4, 4, 1_000_000u32);

        let initial_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();
        field_step(&mut field);
        let final_sum: u64 = field.cells.iter().map(|&v| v as u64).sum();

        assert_eq!(
            initial_sum, final_sum,
            "Mass not conserved at boundary: {} != {}",
            initial_sum, final_sum
        );
    }

    #[test]
    fn test_generation_increments() {
        let mut field = create_field(8, 8, 8, 3);
        assert_eq!(field.generation, 0);

        field_step(&mut field);
        assert_eq!(field.generation, 1);

        field_step(&mut field);
        assert_eq!(field.generation, 2);
    }

    #[test]
    fn test_zero_field_stays_zero() {
        let mut field = create_field(8, 8, 8, 3);

        field_step(&mut field);

        assert!(field.cells.iter().all(|&c| c == 0));
        assert_eq!(field.generation, 1);
    }
}
