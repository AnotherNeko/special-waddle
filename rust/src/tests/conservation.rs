use crate::automaton::cadence::{Cadence, CadenceTree, Gaaabb, SeamPlane, SyncStatus};
use crate::automaton::delta::NeighborKind;
use crate::automaton::field::field_set;
use crate::automaton::incremental::StepController;

/// Phase 9a: Create a tempo seam, prove conservation.
///
/// 32×16×16 field bisected at x=16. Fast zone (x<16) ticks every global tick;
/// slow zone (x>=16) ticks every 4th. Seam face-pairs at x=15↔16 are Buffered
/// with drain_every=4 so mass accumulates in the buffer on fast ticks and drains
/// in bulk on the slow tick. Total field mass must be conserved at every tick.
#[test]
fn test_mass_conservation_tempo_seam() {
    const ROOM_TEMP: u32 = 293_150_000; // µK
    const W: i16 = 32;
    const H: i16 = 16;
    const D: i16 = 16;
    const SEAM_X: i16 = 16;
    const DRAIN_EVERY: u32 = 4;

    let mut ctrl = StepController::new_1(W, H, D, 3, 1);
    ctrl.field.cells.iter_mut().for_each(|c| *c = ROOM_TEMP);
    // Hot spot in fast zone to drive diffusion across the seam.
    field_set(&mut ctrl.field, 8, 8, 8, 1_000_000_000);

    // Replace the default single-leaf cadence with a bisected tree:
    // lo cadence=1 (fast, fires every tick), hi cadence=4 (slow, fires every 4th).
    ctrl.cadence_partition = CadenceTree::new(Gaaabb::new([0, 0, 0], [W, H, D]), Cadence::new(4));
    ctrl.cadence_partition
        .bisect([0, 0, 0], 0, SEAM_X, Cadence::new(1), 0, Cadence::new(4), 0);

    // Register each seam face-pair as Buffered so mass is held in the contract
    // across fast ticks and applied in bulk on the drain tick.
    let idx = |x: i16, y: i16, z: i16| {
        z as usize * H as usize * W as usize + y as usize * W as usize + x as usize
    };
    for z in 0..D {
        for y in 0..H {
            ctrl.delta_overrides.insert(
                (idx(SEAM_X - 1, y, z), idx(SEAM_X, y, z)),
                NeighborKind::Buffered {
                    accumulated: 0,
                    drain_every: DRAIN_EVERY,
                    ticks: 0,
                },
            );
        }
    }

    let initial_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

    for tick in 0..34 {
        let firing = ctrl.cadence_partition.advance();
        ctrl.step_zones_blocking(&firing);

        let tick_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(
            tick_sum,
            initial_sum,
            "Mass conservation failed at tick {} (delta {})",
            tick,
            tick_sum as i64 - initial_sum as i64,
        );
    }
}

/// Phase 9b: Destroy a tempo seam, prove conservation.
///
/// 32×16×16 field, seam at x=16. Starts with lo=cadence 7, hi=cadence 11 (deliberately
/// prime and mismatched to create a non-trivial phase offset). After (7*11+13=)90 ticks the zones
/// are out of phase. `merge()` is then polled each tick using the synchroscope-and-throttle
/// protocol: it nudges hi's cadence by ±1 until accumulators converge, then coarsens.
/// Buffered overrides are removed using the returned SeamPlane. Mass is asserted conserved
/// throughout setup, sync, and 32 post-coarsen ticks.
#[test]
fn test_mass_conservation_seam_teardown() {
    const ROOM_TEMP: u32 = 293_150_000; // µK
    const W: i16 = 32;
    const H: i16 = 16;
    const D: i16 = 16;
    const SEAM_X: i16 = 16;
    const DRAIN_EVERY: u32 = 7; // matches lo cadence so drain lands every lo-cycle

    let mut ctrl = StepController::new_1(W, H, D, 3, 1);
    ctrl.field.cells.iter_mut().for_each(|c| *c = ROOM_TEMP);
    field_set(&mut ctrl.field, 8, 8, 8, 1_000_000_000);

    // Bisect at x=16: lo=cadence 7 (null/reference), hi=cadence 11 (alt, to be synced).
    ctrl.cadence_partition = CadenceTree::new(Gaaabb::new([0, 0, 0], [W, H, D]), Cadence::new(7));
    ctrl.cadence_partition.bisect(
        [0, 0, 0],
        0,
        SEAM_X,
        Cadence::new(7),
        0,
        Cadence::new(11),
        0,
    );

    let seam_plane = SeamPlane {
        axis: 0,
        coord: SEAM_X,
        region: Gaaabb::new([0, 0, 0], [W, H, D]),
    };
    for (lo_idx, hi_idx) in seam_plane.face_pairs(W, H, D) {
        ctrl.delta_overrides.insert(
            (lo_idx, hi_idx),
            NeighborKind::Buffered {
                accumulated: 0,
                drain_every: DRAIN_EVERY,
                ticks: 0,
            },
        );
    }

    let initial_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();

    // Run 13 ticks to establish a non-trivial phase offset between the two zones.
    for tick in 0..90 {
        let firing = ctrl.cadence_partition.advance();
        ctrl.step_zones_blocking(&firing);
        let tick_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(
            tick_sum, initial_sum,
            "9b setup: conservation failed at tick {}",
            tick
        );
    }

    // Sync phase: poll merge() each tick until the seam is dissolved.
    // null=lo zone (cadence 7, unchanged), alt=hi zone (cadence nudged to converge).
    // Buffered overrides are removed when merge() returns Done.
    let null_point = [0i16, 0, 0];
    let alt_point = [SEAM_X, 0, 0];
    let mut dissolved_seam: Option<SeamPlane> = None;
    for tick in 0..200 {
        let firing = ctrl.cadence_partition.advance();
        ctrl.step_zones_blocking(&firing);

        let tick_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(
            tick_sum, initial_sum,
            "9b sync: conservation failed at tick {}",
            tick
        );

        if dissolved_seam.is_none() {
            match ctrl.cadence_partition.merge(null_point, alt_point) {
                SyncStatus::Done(seam) => {
                    dissolved_seam = Some(seam);
                }
                SyncStatus::Syncing => {}
            }
        }
    }

    let seam = dissolved_seam.expect("merge() never converged within 200 ticks");

    // Remove Buffered overrides now that the seam is gone.
    for (lo_idx, hi_idx) in seam.face_pairs(W, H, D) {
        ctrl.delta_overrides.remove(&(lo_idx, hi_idx));
    }

    // Run 32 more ticks as a unified field, asserting conservation throughout.
    for tick in 0..32 {
        let firing = ctrl.cadence_partition.advance();
        ctrl.step_zones_blocking(&firing);
        let tick_sum: u64 = ctrl.field.cells.iter().map(|&v| v as u64).sum();
        assert_eq!(
            tick_sum,
            initial_sum,
            "9b post-coarsen: conservation failed at tick {} (delta {})",
            tick,
            tick_sum as i64 - initial_sum as i64,
        );
    }
}
