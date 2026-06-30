//! Cadence partition: KD-tree over the field that assigns a tick-rate divisor to each region.
//! GAAABB: grid-aligned axis-aligned bounding box.
//!
//! The field starts as a single leaf (one GAAABB covering everything, ambient cadence).
//! `bisect()` splits a leaf into two children at a plane, creating a tempo seam.
//! `coarsen()` merges two same-cadence siblings back into their parent leaf.
//!
//! This module is pure spatial/scheduling logic — no diffusion physics.
//! The caller (StepController) is responsible for registering and draining
//! Buffered NeighborOverride s on the seam face-pairs that bisect/coarsen report back.
//!
//! See GLOSSARY.md: cadence, cadence zone, tempo seam, refinement anchor.

/// Grid-aligned axis-aligned bounding box. Coordinates are in Luanti node-grid space.
/// Both min and max are inclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gaaabb {
    pub min: [i16; 3],
    pub max: [i16; 3],
}

impl Gaaabb {
    pub fn new(min: [i16; 3], max: [i16; 3]) -> Self {
        debug_assert!(min[0] <= max[0] && min[1] <= max[1] && min[2] <= max[2]);
        Gaaabb { min, max }
    }

    pub fn contains(&self, x: i16, y: i16, z: i16) -> bool {
        x >= self.min[0]
            && x <= self.max[0]
            && y >= self.min[1]
            && y <= self.max[1]
            && z >= self.min[2]
            && z <= self.max[2]
    }
}

/// A plane cutting the field at a specific axis and coordinate.
/// Describes the boundary between two cadence zones after a bisect.
///
/// The seam lies between coord-1 and coord along the given axis.
/// Face-pairs at this seam: all (idx_a, idx_b) where axis-coord of idx_a == coord-1
/// and axis-coord of idx_b == coord, within the bounding region.
#[derive(Clone, Debug)]
pub struct SeamPlane {
    /// 0=x, 1=y, 2=z
    pub axis: u8,
    /// First coordinate of the high side (low side is coord-1).
    pub coord: i16,
    /// Bounding region of the seam (the full extent of both zones combined).
    pub region: Gaaabb,
}

/// A node in the cadence KD-tree.
pub enum CadenceNode {
    Leaf {
        region: Gaaabb,
        /// Cadence divisor: 1 = steps every global tick, N = steps every Nth tick.
        divisor: u32,
    },
    Split {
        /// 0=x, 1=y, 2=z
        axis: u8,
        /// First coordinate of the high child.
        coord: i16,
        lo: Box<CadenceNode>,
        hi: Box<CadenceNode>,
    },
}

impl CadenceNode {
    /// Return the cadence divisor of the leaf containing (x, y, z).
    pub fn lookup(&self, x: i16, y: i16, z: i16) -> u32 {
        match self {
            CadenceNode::Leaf { divisor, .. } => *divisor,
            CadenceNode::Split {
                axis,
                coord,
                lo,
                hi,
            } => {
                let v = [x, y, z][*axis as usize];
                if v < *coord {
                    lo.lookup(x, y, z)
                } else {
                    hi.lookup(x, y, z)
                }
            }
        }
    }

    /// Bisect the leaf containing `point` along `axis` at `coord`.
    /// Assigns `lo_divisor` to the low side (axis < coord) and `hi_divisor` to the high side.
    /// Returns the SeamPlane describing the new tempo seam, or None if the point
    /// is not in a leaf (already split at that location).
    pub fn bisect(
        &mut self,
        point: [i16; 3],
        axis: u8,
        coord: i16,
        lo_divisor: u32,
        hi_divisor: u32,
    ) -> Option<SeamPlane> {
        match self {
            CadenceNode::Leaf { region, .. } => {
                if !region.contains(point[0], point[1], point[2]) {
                    return None;
                }
                debug_assert!(
                    coord > region.min[axis as usize] && coord <= region.max[axis as usize],
                    "bisect coord must be strictly inside the region"
                );

                let mut lo_region = region.clone();
                lo_region.max[axis as usize] = coord - 1;

                let mut hi_region = region.clone();
                hi_region.min[axis as usize] = coord;

                let seam = SeamPlane {
                    axis,
                    coord,
                    region: region.clone(),
                };

                *self = CadenceNode::Split {
                    axis,
                    coord,
                    lo: Box::new(CadenceNode::Leaf {
                        region: lo_region,
                        divisor: lo_divisor,
                    }),
                    hi: Box::new(CadenceNode::Leaf {
                        region: hi_region,
                        divisor: hi_divisor,
                    }),
                };

                Some(seam)
            }
            CadenceNode::Split {
                axis: split_axis,
                coord: split_coord,
                lo,
                hi,
            } => {
                let v = point[*split_axis as usize];
                if v < *split_coord {
                    lo.bisect(point, axis, coord, lo_divisor, hi_divisor)
                } else {
                    hi.bisect(point, axis, coord, lo_divisor, hi_divisor)
                }
            }
        }
    }

    /// Attempt to coarsen the split containing `point`: if both children are leaves
    /// with the same divisor, merge them into a single leaf.
    /// Returns the SeamPlane that was dissolved, or None if coarsening was not possible.
    pub fn coarsen(&mut self, point: [i16; 3]) -> Option<SeamPlane> {
        let CadenceNode::Split {
            axis,
            coord,
            lo,
            hi,
        } = self
        else {
            return None;
        };

        let v = point[*axis as usize];
        let target_child = if v < *coord { lo.as_mut() } else { hi.as_mut() };

        // Try to coarsen deeper first.
        if let Some(seam) = target_child.coarsen(point) {
            return Some(seam);
        }

        // Check if both children are leaves with equal divisors.
        if let (
            CadenceNode::Leaf {
                region: lo_region,
                divisor: lo_div,
            },
            CadenceNode::Leaf {
                region: hi_region,
                divisor: hi_div,
            },
        ) = (lo.as_ref(), hi.as_ref())
        {
            if lo_div == hi_div {
                let merged_region = Gaaabb::new(lo_region.min, hi_region.max);
                let seam = SeamPlane {
                    axis: *axis,
                    coord: *coord,
                    region: merged_region.clone(),
                };
                *self = CadenceNode::Leaf {
                    region: merged_region,
                    divisor: *lo_div,
                };
                return Some(seam);
            }
        }

        None
    }

    /// Collect all leaves into a flat list (for scheduler iteration).
    pub fn leaves(&self) -> Vec<&CadenceNode> {
        match self {
            CadenceNode::Leaf { .. } => vec![self],
            CadenceNode::Split { lo, hi, .. } => {
                let mut v = lo.leaves();
                v.extend(hi.leaves());
                v
            }
        }
    }
}

/// The cadence partition for a field. Starts as a single leaf at ambient cadence.
pub struct CadenceTree {
    pub root: CadenceNode,
    pub ambient_divisor: u32,
}

impl CadenceTree {
    /// Create a partition covering `field_region` entirely at `ambient_divisor`.
    pub fn new(field_region: Gaaabb, ambient_divisor: u32) -> Self {
        CadenceTree {
            root: CadenceNode::Leaf {
                region: field_region,
                divisor: ambient_divisor,
            },
            ambient_divisor,
        }
    }

    pub fn lookup(&self, x: i16, y: i16, z: i16) -> u32 {
        self.root.lookup(x, y, z)
    }

    pub fn bisect(
        &mut self,
        point: [i16; 3],
        axis: u8,
        coord: i16,
        lo_divisor: u32,
        hi_divisor: u32,
    ) -> Option<SeamPlane> {
        self.root.bisect(point, axis, coord, lo_divisor, hi_divisor)
    }

    pub fn coarsen(&mut self, point: [i16; 3]) -> Option<SeamPlane> {
        self.root.coarsen(point)
    }

    /// Returns true if the zone containing (x,y,z) should step on this global tick.
    /// Simple modulo check — DDA spreading is a Phase 9c+ concern.
    pub fn should_step(&self, x: i16, y: i16, z: i16, global_tick: u64) -> bool {
        let divisor = self.root.lookup(x, y, z) as u64;
        global_tick % divisor == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_region() -> Gaaabb {
        Gaaabb::new([0, 0, 0], [31, 15, 15])
    }

    #[test]
    fn test_single_leaf_lookup() {
        let tree = CadenceTree::new(field_region(), 4);
        assert_eq!(tree.lookup(0, 0, 0), 4);
        assert_eq!(tree.lookup(31, 15, 15), 4);
    }

    // the following tests only demonstrate basic functionality, and don't even test for conservation of energy before/after operations.

    #[test]
    fn test_bisect_creates_two_zones() {
        let mut tree = CadenceTree::new(field_region(), 4);
        let seam = tree.bisect([0, 0, 0], 0, 16, 1, 4);
        assert!(seam.is_some());
        let seam = seam.unwrap();
        assert_eq!(seam.axis, 0);
        assert_eq!(seam.coord, 16);

        assert_eq!(tree.lookup(0, 0, 0), 1); // left zone: fast
        assert_eq!(tree.lookup(15, 15, 15), 1);
        assert_eq!(tree.lookup(16, 0, 0), 4); // right zone: slow
        assert_eq!(tree.lookup(31, 15, 15), 4);
    }

    #[test]
    fn test_coarsen_restores_single_zone() {
        let mut tree = CadenceTree::new(field_region(), 4);
        tree.bisect([0, 0, 0], 0, 16, 4, 4); // same divisor both sides

        let seam = tree.coarsen([0, 0, 0]);
        assert!(seam.is_some());

        // Should be back to a single leaf.
        assert_eq!(tree.lookup(0, 0, 0), 4);
        assert_eq!(tree.lookup(31, 15, 15), 4);
        assert!(matches!(tree.root, CadenceNode::Leaf { .. }));
    }

    #[test]
    fn test_coarsen_blocked_when_divisors_differ() {
        let mut tree = CadenceTree::new(field_region(), 4);
        tree.bisect([0, 0, 0], 0, 16, 1, 4);

        let seam = tree.coarsen([0, 0, 0]);
        assert!(seam.is_none(), "should not coarsen when divisors differ");
        assert!(matches!(tree.root, CadenceNode::Split { .. }));
    }

    #[test]
    fn test_should_step_respects_divisor() {
        let mut tree = CadenceTree::new(field_region(), 4);
        tree.bisect([0, 0, 0], 0, 16, 1, 4);

        // Fast zone: steps every tick
        assert!(tree.should_step(0, 0, 0, 1));
        assert!(tree.should_step(0, 0, 0, 3));

        // Slow zone: steps only on multiples of 4
        assert!(!tree.should_step(16, 0, 0, 1));
        assert!(!tree.should_step(16, 0, 0, 3));
        assert!(tree.should_step(16, 0, 0, 4));
        assert!(tree.should_step(16, 0, 0, 8));
    }
}
