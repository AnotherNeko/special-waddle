//! Core state structure and helper methods.

/// The internal state of a cellular automaton
pub struct State {
    pub width: i16,
    pub height: i16,
    pub depth: i16,
    pub cells: Vec<u8>, // 0 = dead, 1 = alive
    pub generation: u64,
}

impl State {
    /// Get the linear index for a 3D coordinate
    #[inline]
    pub fn index(&self, x: i16, y: i16, z: i16) -> usize {
        z as usize * self.height as usize * self.width as usize
            + y as usize * self.width as usize
            + x as usize
    }

    /// Count alive neighbors using Moore neighborhood (26 neighbors)
    pub fn count_neighbors(&self, x: i16, y: i16, z: i16) -> u8 {
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

                    // Check bounds
                    if nx >= 0
                        && nx < self.width
                        && ny >= 0
                        && ny < self.height
                        && nz >= 0
                        && nz < self.depth
                    {
                        let idx = self.index(nx, ny, nz);
                        count += self.cells[idx];
                    }
                }
            }
        }

        count
    }
}
