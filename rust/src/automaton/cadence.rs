//! Cadence partition: KD-tree over the field that assigns a tick period to each region.
//! GAAABB: grid-aligned axis-aligned bounding box.
//!
//! Each leaf stores a `cadence: u16` (period) and `accumulator: u16` (running counter).
//! Every call to `advance()` increments all leaf accumulators by one. When
//! `accumulator >= cadence` the leaf steps and subtracts cadence from accumulator
//! (not a reset — the remainder carries forward, preventing long-term drift).
//! This is the fractional-accumulator pattern used in hardware timer compare registers
//! (e.g. MSP432 Timer_A): additive, drift-free, no modulo arithmetic.
//!
//! Phase is a consequence of the initial accumulator value. Two leaves with equal
//! cadence but different accumulators are out of phase and cannot be coarsened until
//! their accumulators match.
//!
//! The field starts as a single leaf (one GAAABB covering everything, ambient cadence).
//! `bisect()` splits a leaf into two children at a plane, creating a tempo seam.
//! `coarsen()` merges two same-cadence, same-accumulator siblings back into their parent.
//!
//! This module is pure spatial/scheduling logic — no diffusion physics.
//! The caller (StepController) is responsible for registering and draining
//! Buffered NeighborOverrides on the seam face-pairs that bisect/coarsen report back.
//!
//! See GLOSSARY.md: cadence, cadence zone, tempo seam, refinement anchor, phase rotation meter.

use std::num::NonZeroU16;

/// A validated cadence period: number of global ticks between simulation steps for a zone.
///
/// Must be ≥ 1 (enforced by NonZeroU16). Additionally, at the point of use,
/// `conductivity * cadence` must be less than the diffusion divisor — values that violate
/// this stability bound will be caught by `compute_flow`'s debug_assert and cause
/// underflow. The maximum safe cadence for a given `diffusion_rate` r is `7 * 2^r - 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cadence(NonZeroU16);

impl Cadence {
    pub fn new(value: u16) -> Self {
        Cadence(NonZeroU16::new(value).expect("cadence must be >= 1"))
    }

    pub fn get(self) -> u16 {
        self.0.get()
    }
}

/// Grid-aligned axis-aligned bounding box. Coordinates are in Luanti node-grid space.
/// min is inclusive, max is exclusive (half-open interval [min, max)).
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
            && x < self.max[0]
            && y >= self.min[1]
            && y < self.max[1]
            && z >= self.min[2]
            && z < self.max[2]
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

impl SeamPlane {
    /// Enumerate all (lo_idx, hi_idx) pairs on the seam face for a field of given dimensions.
    /// Row-major indexing: idx = z * height * width + y * width + x.
    pub fn face_pairs(&self, width: i16, height: i16, depth: i16) -> Vec<(usize, usize)> {
        let stride = |x: i16, y: i16, z: i16| {
            z as usize * height as usize * width as usize + y as usize * width as usize + x as usize
        };
        let r = &self.region;
        let mut pairs = Vec::new();
        match self.axis {
            0 => {
                let x_lo = self.coord - 1;
                let x_hi = self.coord;
                for z in r.min[2]..r.max[2] {
                    for y in r.min[1]..r.max[1] {
                        pairs.push((stride(x_lo, y, z), stride(x_hi, y, z)));
                    }
                }
            }
            1 => {
                let y_lo = self.coord - 1;
                let y_hi = self.coord;
                for z in r.min[2]..r.max[2] {
                    for x in r.min[0]..r.max[0] {
                        pairs.push((stride(x, y_lo, z), stride(x, y_hi, z)));
                    }
                }
            }
            _ => {
                let z_lo = self.coord - 1;
                let z_hi = self.coord;
                for y in r.min[1]..r.max[1] {
                    for x in r.min[0]..r.max[0] {
                        pairs.push((stride(x, y, z_lo), stride(x, y, z_hi)));
                    }
                }
            }
        }
        pairs
    }
}

/// A node in the cadence KD-tree.
pub enum CadenceNode {
    Leaf {
        region: Gaaabb,
        /// Period in global ticks. Cadence(1) steps every tick, Cadence(N) every N ticks.
        cadence: Cadence,
        /// Fractional accumulator. Incremented each tick; when >= cadence, the leaf
        /// steps and cadence is subtracted (remainder preserved for drift-free timing).
        /// Initial value determines phase. Stays u16: zero is valid, Cadence is not.
        accumulator: u16,
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
    /// Return the cadence period of the leaf containing (x, y, z).
    pub fn lookup_cadence(&self, x: i16, y: i16, z: i16) -> Cadence {
        match self {
            CadenceNode::Leaf { cadence, .. } => *cadence,
            CadenceNode::Split {
                axis,
                coord,
                lo,
                hi,
            } => {
                let v = [x, y, z][*axis as usize];
                if v < *coord {
                    lo.lookup_cadence(x, y, z)
                } else {
                    hi.lookup_cadence(x, y, z)
                }
            }
        }
    }

    /// Increment all leaf accumulators by one. Appends (GAAABB, cadence) for every
    /// leaf that fires. The cadence value is the dt (time step in global ticks) to
    /// pass to compute_flow so the physical time constant is cadence-invariant.
    pub fn advance(&mut self, stepping: &mut Vec<(Gaaabb, Cadence)>) {
        match self {
            CadenceNode::Leaf {
                region,
                cadence,
                accumulator,
            } => {
                *accumulator += 1;
                if *accumulator >= cadence.get() {
                    *accumulator -= cadence.get();
                    stepping.push((region.clone(), *cadence));
                }
            }
            CadenceNode::Split { lo, hi, .. } => {
                lo.advance(stepping);
                hi.advance(stepping);
            }
        }
    }

    /// Bisect the leaf containing `point` along `axis` at `coord`.
    /// Returns the SeamPlane describing the new tempo seam, or None if the point
    /// is not in a leaf.
    pub fn bisect(
        &mut self,
        point: [i16; 3],
        axis: u8,
        coord: i16,
        lo_cadence: Cadence,
        lo_accumulator: u16,
        hi_cadence: Cadence,
        hi_accumulator: u16,
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
                lo_region.max[axis as usize] = coord;

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
                        cadence: lo_cadence,
                        accumulator: lo_accumulator,
                    }),
                    hi: Box::new(CadenceNode::Leaf {
                        region: hi_region,
                        cadence: hi_cadence,
                        accumulator: hi_accumulator,
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
                    lo.bisect(
                        point,
                        axis,
                        coord,
                        lo_cadence,
                        lo_accumulator,
                        hi_cadence,
                        hi_accumulator,
                    )
                } else {
                    hi.bisect(
                        point,
                        axis,
                        coord,
                        lo_cadence,
                        lo_accumulator,
                        hi_cadence,
                        hi_accumulator,
                    )
                }
            }
        }
    }

    /// Attempt to coarsen the split containing `point`: if both children are leaves
    /// with equal cadence AND equal accumulator (in phase), merge them.
    ///
    /// Returns the SeamPlane that was dissolved, or None if coarsening was not possible.
    ///
    /// TODO(phase rotation meter): Merging two leaves whose accumulators are out of phase
    /// requires a controller looking at a synchroscope that nudges one zone's cadence until accumulators converge,
    /// then coarsens. Do not brute-force a merge when accumulators differ — the resulting
    /// phase discontinuity will inject or destroy mass at the moment of merge.
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

        if let Some(seam) = target_child.coarsen(point) {
            return Some(seam);
        }

        if let (
            CadenceNode::Leaf {
                region: lo_region,
                cadence: lo_cad,
                accumulator: lo_acc,
            },
            CadenceNode::Leaf {
                region: hi_region,
                cadence: hi_cad,
                accumulator: hi_acc,
            },
        ) = (lo.as_ref(), hi.as_ref())
        {
            if lo_cad == hi_cad && lo_acc == hi_acc {
                let merged_region = Gaaabb::new(lo_region.min, hi_region.max);
                let seam = SeamPlane {
                    axis: *axis,
                    coord: *coord,
                    region: merged_region.clone(),
                };
                *self = CadenceNode::Leaf {
                    region: merged_region,
                    cadence: *lo_cad,
                    accumulator: *lo_acc,
                };
                return Some(seam);
            }
        }

        None
    }

    /// Set the cadence of the leaf containing `point` to `new_cadence`.
    /// The accumulator is preserved so the caller can decide when to coarsen.
    /// Returns true if a leaf was found and updated.
    pub fn set_cadence(&mut self, point: [i16; 3], new_cadence: Cadence) -> bool {
        match self {
            CadenceNode::Leaf { cadence, .. } => {
                *cadence = new_cadence;
                true
            }
            CadenceNode::Split {
                axis,
                coord,
                lo,
                hi,
            } => {
                let v = point[*axis as usize];
                if v < *coord {
                    lo.set_cadence(point, new_cadence)
                } else {
                    hi.set_cadence(point, new_cadence)
                }
            }
        }
    }

    /// Return the (cadence, accumulator) of the leaf containing `point`.
    fn leaf_state(&self, point: [i16; 3]) -> (Cadence, u16) {
        match self {
            CadenceNode::Leaf { cadence, accumulator, .. } => (*cadence, *accumulator),
            CadenceNode::Split { axis, coord, lo, hi } => {
                let v = point[*axis as usize];
                if v < *coord { lo.leaf_state(point) } else { hi.leaf_state(point) }
            }
        }
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

/// Result of a `CadenceTree::merge()` poll.
pub enum SyncStatus {
    /// Zones are not yet in phase; cadence adjustment applied or still drifting.
    Syncing,
    /// Zones converged and were coarsened. Contains the dissolved seam.
    Done(SeamPlane),
}

/// The cadence partition for a field. Starts as a single leaf at ambient cadence.
pub struct CadenceTree {
    pub root: CadenceNode,
    pub ambient_cadence: Cadence,
}

impl CadenceTree {
    /// Create a partition covering `field_region` at `ambient_cadence`.
    /// Accumulator initialised to zero: first step fires after one full period.
    pub fn new(field_region: Gaaabb, ambient_cadence: Cadence) -> Self {
        CadenceTree {
            root: CadenceNode::Leaf {
                region: field_region,
                cadence: ambient_cadence,
                accumulator: 0,
            },
            ambient_cadence,
        }
    }

    pub fn lookup_cadence(&self, x: i16, y: i16, z: i16) -> Cadence {
        self.root.lookup_cadence(x, y, z)
    }

    /// Advance all leaf accumulators by one global tick.
    /// Returns (GAAABB, Cadence) for each zone that fires this tick. The Cadence
    /// value is the dt (time step in global ticks) to pass to compute_flow.
    pub fn advance(&mut self) -> Vec<(Gaaabb, Cadence)> {
        let mut stepping = Vec::new();
        self.root.advance(&mut stepping);
        stepping
    }

    pub fn bisect(
        &mut self,
        point: [i16; 3],
        axis: u8,
        coord: i16,
        lo_cadence: Cadence,
        lo_accumulator: u16,
        hi_cadence: Cadence,
        hi_accumulator: u16,
    ) -> Option<SeamPlane> {
        self.root.bisect(
            point,
            axis,
            coord,
            lo_cadence,
            lo_accumulator,
            hi_cadence,
            hi_accumulator,
        )
    }

    pub fn coarsen(&mut self, point: [i16; 3]) -> Option<SeamPlane> {
        self.root.coarsen(point)
    }

    /// Set the cadence of the leaf containing `point`.
    pub fn set_cadence(&mut self, point: [i16; 3], new_cadence: Cadence) -> bool {
        self.root.set_cadence(point, new_cadence)
    }

    /// Synchronize two zones for coarsening using the synchroscope-and-throttle protocol.
    ///
    /// `null_point`: a point in the reference zone (cadence is the merge target, unchanged).
    /// `alt_point`: a point in the zone being synchronized (cadence is nudged ±1).
    ///
    /// Call once per tick after `advance()`. On the first call (alt and null cadences equal,
    /// diff ≠ 0), nudges alt cadence by ±1 based on the sign of (alt_acc − null_acc).
    /// On subsequent calls, polls until accumulators match; when they do, restores alt
    /// cadence to null cadence and coarsens. Returns `Done(seam)` on completion.
    ///
    /// The cadence nudge creates a known phase drift — caller need not compute the rate.
    pub fn merge(&mut self, null_point: [i16; 3], alt_point: [i16; 3]) -> SyncStatus {
        let (null_cad, null_acc) = self.root.leaf_state(null_point);
        let (alt_cad, alt_acc) = self.root.leaf_state(alt_point);

        let diff = alt_acc as i32 - null_acc as i32;

        if diff == 0 {
            // Restore alt cadence to null cadence if a nudge was applied, then coarsen.
            if alt_cad != null_cad {
                self.root.set_cadence(alt_point, null_cad);
            }
            if let Some(seam) = self.root.coarsen(null_point) {
                return SyncStatus::Done(seam);
            }
            // coarsen failed (e.g. not siblings) — fall through to Syncing
        } else if alt_cad == null_cad {
            // First call: apply a one-tick nudge to start the drift.
            let nudged = if diff > 0 {
                Cadence::new(null_cad.get() + 1) // alt is ahead, slow it down
            } else {
                Cadence::new(null_cad.get().saturating_sub(1).max(1)) // alt is behind, speed it up
            };
            self.root.set_cadence(alt_point, nudged);
        }
        // If alt_cad != null_cad and diff != 0: still drifting, nothing to do.

        SyncStatus::Syncing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_region() -> Gaaabb {
        Gaaabb::new([0, 0, 0], [32, 16, 16])
    }

    fn c(n: u16) -> Cadence {
        Cadence::new(n)
    }

    #[test]
    fn test_single_leaf_lookup() {
        let tree = CadenceTree::new(field_region(), c(4));
        assert_eq!(tree.lookup_cadence(0, 0, 0), c(4));
        assert_eq!(tree.lookup_cadence(31, 15, 15), c(4));
    }

    #[test]
    fn test_advance_fires_on_period() {
        let mut tree = CadenceTree::new(field_region(), c(4));
        // accumulator: 0 → 1, 2, 3, 4>=4 step (acc becomes 0).
        assert!(tree.advance().is_empty()); // acc=1
        assert!(tree.advance().is_empty()); // acc=2
        assert!(tree.advance().is_empty()); // acc=3
        let stepping = tree.advance(); // acc=4>=4, step, acc=0
        assert_eq!(stepping.len(), 1);
        assert_eq!(stepping[0].0, field_region());
        assert_eq!(stepping[0].1, c(4)); // dt = cadence
        assert!(tree.advance().is_empty()); // acc=1, cycle repeats
    }

    #[test]
    fn test_cadence_1_steps_every_tick() {
        let mut tree = CadenceTree::new(field_region(), c(1));
        for _ in 0..8 {
            assert_eq!(tree.advance().len(), 1);
        }
    }

    #[test]
    fn test_phase_offset_via_initial_accumulator() {
        let mut tree = CadenceTree::new(field_region(), c(4));
        // Bisect with lo starting mid-period (acc=2): fires after 2 more ticks.
        // hi starts fresh (acc=0): fires after 4 ticks.
        tree.bisect([0, 0, 0], 0, 16, c(4), 2, c(4), 0);

        // Tick 1: lo acc=3 (no), hi acc=1 (no)
        assert!(tree.advance().is_empty());
        // Tick 2: lo acc=4>=4 → step (acc=0); hi acc=2 (no)
        let s2 = tree.advance();
        assert_eq!(s2.len(), 1);
        assert_eq!(s2[0].0.max[0], 16); // lo region
                                        // Tick 3: lo acc=1 (no), hi acc=3 (no)
        assert!(tree.advance().is_empty());
        // Tick 4: lo acc=2 (no), hi acc=4>=4 → step
        let s4 = tree.advance();
        assert_eq!(s4.len(), 1);
        assert_eq!(s4[0].0.min[0], 16); // hi region
    }

    #[test]
    fn test_two_zones_different_cadences() {
        let mut tree = CadenceTree::new(field_region(), c(4));
        // lo: cadence=1 (fast), hi: cadence=4 (slow), both acc=0.
        tree.bisect([0, 0, 0], 0, 16, c(1), 0, c(4), 0);

        // Ticks 1-3: only fast fires.
        for _ in 0..3 {
            let s = tree.advance();
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].0.max[0], 16); // lo
        }
        // Tick 4: both fire.
        assert_eq!(tree.advance().len(), 2);
    }

    #[test]
    fn test_coarsen_when_in_phase() {
        let mut tree = CadenceTree::new(field_region(), c(4));
        tree.bisect([0, 0, 0], 0, 16, c(4), 0, c(4), 0);

        let seam = tree.coarsen([0, 0, 0]);
        assert!(seam.is_some());
        assert_eq!(tree.lookup_cadence(0, 0, 0), c(4));
        assert_eq!(tree.lookup_cadence(31, 15, 15), c(4));
        assert!(matches!(tree.root, CadenceNode::Leaf { .. }));
    }

    #[test]
    fn test_coarsen_blocked_when_cadences_differ() {
        let mut tree = CadenceTree::new(field_region(), c(4));
        tree.bisect([0, 0, 0], 0, 16, c(1), 0, c(4), 0);
        assert!(tree.coarsen([0, 0, 0]).is_none());
    }

    #[test]
    fn test_coarsen_blocked_when_out_of_phase() {
        let mut tree = CadenceTree::new(field_region(), c(4));
        // Same cadence, different accumulators.
        tree.bisect([0, 0, 0], 0, 16, c(4), 1, c(4), 3);
        assert!(tree.coarsen([0, 0, 0]).is_none());
    }

    #[test]
    fn test_remainder_carries_forward() {
        // cadence=3, verify it fires at ticks 3, 6, 9... not drifting.
        let mut tree = CadenceTree::new(field_region(), c(3));
        let firing_ticks: Vec<usize> = (1..=12).filter(|_| !tree.advance().is_empty()).collect();
        assert_eq!(firing_ticks, vec![3, 6, 9, 12]);
    }
}
