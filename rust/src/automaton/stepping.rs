//! Cellular automaton stepping with B4/S4 rules.

use super::grid::{count_neighbors, index_of};
use crate::state::State;

/// Step the automaton forward by one generation using B4/S4 rules.
///
/// B4/S4 rules:
/// - Birth: A dead cell with exactly 4 neighbors becomes alive
/// - Survival: An alive cell with exactly 4 neighbors survives
/// - Moore neighborhood: 26 neighbors (3x3x3 cube excluding center)
pub fn step_automaton(state: &mut State) {
    if state.cells.is_empty() {
        return;
    }

    let mut next_cells = vec![0; state.cells.len()];

    for z in 0..state.depth {
        for y in 0..state.height {
            for x in 0..state.width {
                let neighbors = count_neighbors(state, x, y, z);
                let idx = index_of(state, x, y, z);

                // B4/S4 rule: Birth on 4, Survival on 4
                next_cells[idx] = if neighbors == 4 { 1 } else { 0 };
            }
        }
    }

    state.cells = next_cells;
    state.generation += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automaton::grid::create_grid;

    #[test]
    fn test_step_b4s4_basic() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 8, 8, 8);

        // Create a cross pattern: center + 4 orthogonal neighbors
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

        // Count initial alive cells
        let initial_alive = state.cells.iter().filter(|&&c| c == 1).count();
        assert_eq!(initial_alive, 5);

        step_automaton(&mut state);

        // Center had 4 neighbors, should survive
        assert_eq!(state.cells[index_of(&state, 4, 4, 4)], 1);

        // Each edge had 2 neighbors, should die
        assert_eq!(state.cells[index_of(&state, 3, 4, 4)], 0);
        assert_eq!(state.cells[index_of(&state, 5, 4, 4)], 0);
        assert_eq!(state.cells[index_of(&state, 4, 3, 4)], 0);
        assert_eq!(state.cells[index_of(&state, 4, 5, 4)], 0);

        // Generation incremented
        assert_eq!(state.generation, 1);
    }

    #[test]
    fn test_step_generation_increments() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 4, 4, 4);

        assert_eq!(state.generation, 0);
        step_automaton(&mut state);
        assert_eq!(state.generation, 1);
        step_automaton(&mut state);
        assert_eq!(state.generation, 2);
    }

    #[test]
    fn test_step_empty_grid_stays_empty() {
        let mut state = State {
            width: 0,
            height: 0,
            depth: 0,
            cells: Vec::new(),
            generation: 0,
        };

        create_grid(&mut state, 4, 4, 4);

        step_automaton(&mut state);
        assert!(state.cells.iter().all(|&c| c == 0));
        assert_eq!(state.generation, 1);
    }
}
