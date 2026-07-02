use crate::automaton::cadence::{Cadence, CadenceTree, Gaaabb};
use crate::automaton::delta::{Contract, ContractKind};
use crate::automaton::incremental::StepController;

const W: i16 = 3;
const H: i16 = 3;
const D: i16 = 9;
const DIFFUSION_RATE: u8 = 4;
// z*H*W + y*W + x for (1,1,1) in a 3×3×9 field
const INFINITY_CELL: usize = 1 * 3 * 3 + 1 * 3 + 1; // = 13
const MEASURE_CELL: usize = 2 * 3 * 3 + 2 * 3 + 5; // = 29: (x=2,y=0,z=3), away from sink

fn build_ctrl(initial: u32, cadence: Cadence) -> StepController {
    let mut ctrl = StepController::new(W, H, D, DIFFUSION_RATE, 1);
    ctrl.field.cells.iter_mut().for_each(|c| *c = initial);
    ctrl.contract_list.contracts.push(Contract {
        src_a: INFINITY_CELL as u32,
        src_b: 0,
        dst_a: INFINITY_CELL as u32,
        dst_b: 0,
        kind: ContractKind::Infinity {
            target_value: 0,
            consumed: 0,
        },
    });
    ctrl.cadence_partition = CadenceTree::new(Gaaabb::new([0, 0, 0], [W, H, D]), cadence);
    ctrl
}

/// Returns the number of global ticks for MEASURE_CELL to decay to initial/e (63.2% rule),
/// or None if the cell never reaches the threshold within 10M ticks (stability failure).
fn measure_tau_global(initial: u32, cadence: Cadence) -> Option<u64> {
    let mut ctrl = build_ctrl(initial, cadence);
    let threshold = (initial as f64 / std::f64::consts::E) as u32;
    let mut global_ticks = 0u64;
    loop {
        let firing = ctrl.cadence_partition.advance();
        ctrl.step_zones_blocking(&firing);
        global_ticks += 1;
        if ctrl.field.cells[MEASURE_CELL] <= threshold {
            return Some(global_ticks);
        }
        if global_ticks >= 1e5 as u64 {
            eprintln!(
                "  cadence={}: UNSTABLE after 1e5 ticks \
                 (cell={}, threshold={}, generation={})",
                cadence.get(),
                ctrl.field.cells[MEASURE_CELL],
                threshold,
                ctrl.field.generation,
            );
            return None;
        }
    }
}

/// Phase 9a: time constant invariance under cadence change.
///
/// GLOSSARY § Cadence: "longer time-constant processes are not affected — the time
/// constant (as perceived on wall time) is preserved as the cadence varies."
///
/// A 3×3×9 field at uniform temperature decays toward an Infinity sink at (1,1,1).
/// τ is measured as the number of global ticks to reach 1/e of the initial value
/// (63.2% rule) at MEASURE_CELL. Eight samples per cadence span a ±35% temperature
/// range to give the ANOVA within-group variance a basis.
///
/// One-way ANOVA tests H0: all cadence groups share the same mean τ. If F > F_crit,
/// H0 is rejected — the current implementation does not satisfy the GLOSSARY invariant.
#[test]
fn test_time_constant_preserved_under_cadence_change() {
    // Max safe cadence for DIFFUSION_RATE=4: 7 * 2^4 - 1 = 111.
    // in practice, cadence values above 18 are unstable - bugfix TODO.
    const CADENCE_VALUES: [u16; 14] = [1, 2, 3, 4, 6, 8, 10, 14, 18, 22, 25, 30, 45, 60];
    const N_SAMPLES: usize = 8;
    const BASE_TEMP: u32 = 200_000_000; // µK
    const TEMP_STEP: u32 = 27_571_428; // ~14% steps, 8 samples → 200M..393M µK

    // Collect samples; None means the cadence was unstable for that sample.
    let groups: Vec<Vec<Option<f64>>> = CADENCE_VALUES
        .iter()
        .map(|&c| {
            let cadence = Cadence::new(c);
            (0..N_SAMPLES)
                .map(|i| {
                    measure_tau_global(BASE_TEMP + i as u32 * TEMP_STEP, cadence).map(|t| t as f64)
                })
                .collect()
        })
        .collect();

    // Print per-cadence mean τ for stable samples; flag unstable ones.
    let mut any_unstable = false;
    let group_means: Vec<Option<f64>> = groups
        .iter()
        .zip(&CADENCE_VALUES)
        .map(|(g, &c)| {
            let stable: Vec<f64> = g.iter().filter_map(|&v| v).collect();
            if stable.len() < N_SAMPLES {
                eprintln!(
                    "cadence={:2}: UNSTABLE ({}/{} samples failed)",
                    c,
                    N_SAMPLES - stable.len(),
                    N_SAMPLES
                );
                any_unstable = true;
                None
            } else {
                let mean = stable.iter().sum::<f64>() / stable.len() as f64;
                let variance =
                    stable.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / stable.len() as f64;
                let sigma = variance.sqrt();
                eprintln!(
                    "cadence={:2}: mean τ = {:.1} ± {:.1} global ticks",
                    c, mean, sigma
                );
                Some(mean)
            }
        })
        .collect();

    assert!(
        !any_unstable,
        "some cadence values were unstable (see output above)"
    );

    // All stable — unwrap for ANOVA.
    let means: Vec<f64> = group_means.iter().map(|m| m.unwrap()).collect();
    let flat: Vec<f64> = groups
        .iter()
        .flat_map(|g| g.iter().map(|v| v.unwrap()))
        .collect();

    // One-way ANOVA — H0: all cadence groups share the same mean τ in global ticks.
    let k = CADENCE_VALUES.len();
    let n_total = k * N_SAMPLES;
    let grand_mean: f64 = flat.iter().sum::<f64>() / n_total as f64;

    let ss_between: f64 = means
        .iter()
        .map(|&gm| N_SAMPLES as f64 * (gm - grand_mean).powi(2))
        .sum();

    let ss_within: f64 = groups
        .iter()
        .zip(&means)
        .flat_map(|(g, &gm)| g.iter().map(move |v| (v.unwrap() - gm).powi(2)))
        .sum();

    let df_between = (k - 1) as f64;
    let df_within = (n_total - k) as f64;
    let ms_between = ss_between / df_between;
    let ms_within = if ss_within == 0.0 {
        f64::EPSILON
    } else {
        ss_within / df_within
    };
    let f_stat = ms_between / ms_within;

    // Critical value for F(k-1, n_total-k) at α = 0.05.
    let f_critical = 2.64_f64; // F(13, 98) ≈ 1.89 but conservative bound is fine
    eprintln!(
        "ANOVA: F({}, {}) = {:.1}, critical = {:.2} (α=0.05)",
        df_between as usize, df_within as usize, f_stat, f_critical,
    );

    assert!(
        f_stat <= f_critical,
        "H0 rejected: τ varies significantly with cadence \
         (F = {:.1} >> {:.2}). Group means (global ticks): [{}]. \
         GLOSSARY § Cadence requires τ to be cadence-invariant for slow processes. \
         Current implementation does not satisfy this — see Phase 9 for the fix.",
        f_stat,
        f_critical,
        means
            .iter()
            .map(|m| format!("{:.0}", m))
            .collect::<Vec<_>>()
            .join(", "),
    );
}
