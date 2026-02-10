//! Grid initialization and cell access helpers.

use crate::state::State;

/// Initialize a grid with the given dimensions.
pub fn create_grid(state: &mut State, width: i16, height: i16, depth: i16) {
    let size = (width as usize) * (height as usize) * (depth as usize);
    state.width = width;
    state.height = height;
    state.depth = depth;
    state.cells = vec![0; size];
    state.generation = 0;
}

/// Calculate the linear index for a 3D coordinate.
#[inline]
pub fn index_of(state: &State, x: i16, y: i16, z: i16) -> usize {
    z as usize * state.height as usize * state.width as usize
        + y as usize * state.width as usize
        + x as usize
}

/// Check if coordinates are within grid bounds.
#[inline]
pub fn in_bounds(state: &State, x: i16, y: i16, z: i16) -> bool {
    x >= 0 && x < state.width && y >= 0 && y < state.height && z >= 0 && z < state.depth
}

/// Count alive neighbors using Moore neighborhood (26 neighbors).
pub fn count_neighbors(state: &State, x: i16, y: i16, z: i16) -> u8 {
    let mut count = 0;

    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                // Skip the center cell
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }

                let nx = x + dx;
                let ny = y + dy;
                let nz = z + dz;

                if in_bounds(state, nx, ny, nz) {
                    let idx = index_of(state, nx, ny, nz);
                    count += state.cells[idx];
                }
            }
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_grid() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 8, 8, 8);
        assert_eq!(state.width, 8);
        assert_eq!(state.height, 8);
        assert_eq!(state.depth, 8);
        assert_eq!(state.cells.len(), 512);
        assert_eq!(state.generation, 0);
        assert!(state.cells.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_index_of() {
        let state = State {
            width: 4,
            height: 4,
            depth: 4,
            cells: vec![0; 64],
            generation: 0,
        };

        // First cell
        assert_eq!(index_of(&state, 0, 0, 0), 0);
        // Last cell
        assert_eq!(index_of(&state, 3, 3, 3), 63);
        // Various cells
        assert_eq!(index_of(&state, 1, 0, 0), 1);
        assert_eq!(index_of(&state, 0, 1, 0), 4);
        assert_eq!(index_of(&state, 0, 0, 1), 16);
    }

    #[test]
    fn test_in_bounds() {
        let state = State {
            width: 4,
            height: 4,
            depth: 4,
            cells: vec![0; 64],
            generation: 0,
        };

        // Valid bounds
        assert!(in_bounds(&state, 0, 0, 0));
        assert!(in_bounds(&state, 3, 3, 3));
        assert!(in_bounds(&state, 2, 2, 2));

        // Out of bounds
        assert!(!in_bounds(&state, -1, 0, 0));
        assert!(!in_bounds(&state, 4, 0, 0));
        assert!(!in_bounds(&state, 0, -1, 0));
        assert!(!in_bounds(&state, 0, 4, 0));
        assert!(!in_bounds(&state, 0, 0, -1));
        assert!(!in_bounds(&state, 0, 0, 4));
    }

    #[test]
    fn test_count_neighbors() {
        let mut state = State {
            width: 8,
            height: 8,
            depth: 8,
            cells: vec![0; 512],
            generation: 0,
        };

        // Set up a cross pattern: center + 4 neighbors
        let idx_center = index_of(&state, 4, 4, 4);
        state.cells[idx_center] = 1;

        let idx_left = index_of(&state, 3, 4, 4);
        state.cells[idx_left] = 1;

        let idx_right = index_of(&state, 5, 4, 4);
        state.cells[idx_right] = 1;

        let idx_front = index_of(&state, 4, 3, 4);
        state.cells[idx_front] = 1;

        let idx_back = index_of(&state, 4, 5, 4);
        state.cells[idx_back] = 1;

        // Center should have 4 neighbors (left, right, front, back)
        assert_eq!(count_neighbors(&state, 4, 4, 4), 4);

        // Each edge should have 3 neighbors (center + 2 orthogonal edges)
        assert_eq!(count_neighbors(&state, 3, 4, 4), 3); // center, front, back
        assert_eq!(count_neighbors(&state, 5, 4, 4), 3); // center, front, back
        assert_eq!(count_neighbors(&state, 4, 3, 4), 3); // center, left, right
        assert_eq!(count_neighbors(&state, 4, 5, 4), 3); // center, left, right

        // Far cell should have 0 neighbors
        assert_eq!(count_neighbors(&state, 0, 0, 0), 0);
    }
}
