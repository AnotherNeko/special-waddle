//! Delta contract abstraction for cell-pair flow computation.
//!
//! `DeltaKind` is the Phase 8B hybrid enum. The modal fast path — used for
//! 99.99% of pairs — bypasses this enum entirely. Only pairs registered in
//! `DeltaOverrides` reach here.

// Per-pair flow contract. `Modal` is the degenerate case (same as inline
// compute_flow). Future variants (Persistent, Remote) extend without changing
// the hot path.
pub enum DeltaKind {
    // Gradient diffusion, same formula as the inline fast path. Registered
    // overrides may be Modal to allow future wrapping without hot-path cost.
    Modal,
    // Same computation as Modal, but records each flow value for diagnostics.
    Logged { log: Vec<i64> },
    // A force mirror, kinda prevents flows.
    Mirror,
    // Void violates conservation of mass intentionally.
    Void,
    // future types: Persistent, Remote
}

impl DeltaKind {
    // Compute and record the flow for this pair. `compute_fn` is the modal
    // formula; all variants call it so logged flows are directly comparable.
    pub fn apply(
        &mut self,
        gradient: i64,
        conductivity: i64,
        divisor: i64,
        remainder_acc: &mut i64,
        compute_fn: impl Fn(i64, i64, i64, &mut i64) -> i64,
    ) -> i64 {
        let flow = compute_fn(gradient, conductivity, divisor, remainder_acc);
        match self {
            DeltaKind::Modal => flow,
            DeltaKind::Logged { log } => {
                log.push(flow);
                flow
            }
            // Mirror blocks flow: conductivity=0 means gradient*0/divisor = 0.
            DeltaKind::Mirror => 0,
            // Void swallows mass: apply flow to owner but not neighbor.
            // Conservation is intentionally violated (mass sink).
            DeltaKind::Void => flow,
        }
    }

    pub fn new_logged() -> Self {
        DeltaKind::Logged { log: Vec::new() }
    }

    pub fn log(&self) -> Option<&[i64]> {
        match self {
            DeltaKind::Logged { log } => Some(log),
            _ => None,
        }
    }
}

// Sparse map from cell-pair `(owner_idx, neighbor_idx)` to a contract.
// The owner index is always the cell that owns the pair in the
// owner-writes-positive scheme (i.e., the lower-xyz cell of the two).
pub type DeltaOverrides = std::collections::HashMap<(usize, usize), DeltaKind>;
