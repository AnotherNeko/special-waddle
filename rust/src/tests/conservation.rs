use crate::automaton::cadence::{Cadence, CadenceTree, Gaaabb};
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

    let mut ctrl = StepController::new(W, H, D, 3, 1);
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
