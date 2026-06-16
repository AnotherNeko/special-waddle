//! Delta contract abstraction for cell-pair flow computation.
//!
//! `DeltaKind` is the Phase 8B hybrid enum. The modal fast path — used for
//! 99.99% of pairs — bypasses this enum entirely. Only pairs registered in
//! `DeltaOverrides` reach here.
//!
//! # Contract taxonomy (with ONI analogues)
//!
//! - `Modal`  — normal diffusion. The common case; registered overrides may be
//!   Modal to allow future wrapping without touching the hot path.
//!
//! - `Mirror` — perfectly insulating boundary. No substance crosses the pair
//!   regardless of gradient. ONI analogue: **neutronium**, the unobtainable
//!   map-edge tile that hard-walls all heat and mass transfer.
//!
//! - `Void`   — mass sink. Substance flows out of the owner and disappears;
//!   the neighbor index is a phantom that never receives anything. Conservation
//!   is intentionally violated. ONI analogue: **space** — a tile open to vacuum
//!   beyond the asteroid boundary. Gas/liquid that reaches a space-adjacent cell
//!   exits the simulation permanently.
//!
//! - `Remote` (future) — cross-server portal. Like `Void`, the neighbor index
//!   is not a local cell — it lives on a different game server in a Clusterio
//!   cluster. The flow is serialized and forwarded over the network; the remote
//!   server applies the matching inbound delta to its local cell. From each
//!   server's perspective the pair looks like a Void until the network ACK arrives.

/// Per-pair flow contract. `Modal` is the degenerate case (same as inline
/// `compute_flow`). Non-modal variants are sparse and looked up only when
/// `cell_has_override` flags the owner cell, keeping the hot path branchless.
pub enum DeltaKind {
    /// Gradient diffusion identical to the inline fast path.
    Modal,
    /// Same as Modal but records each flow value for diagnostics.
    Logged { log: Vec<i64> },
    /// Perfectly insulating boundary — zero flow regardless of gradient.
    /// Use for map-edge tiles that must never exchange substance (neutronium).
    Mirror,
    /// Mass sink — owner loses substance, neighbor receives nothing.
    /// Use for cells adjacent to out-of-bounds vacuum (open space, hull breach).
    Void,
    // Remote: portal to a field on another game server (Clusterio). Like Void,
    // the neighbor index is non-local; the flow is forwarded over the network.
    // Needs async accumulation design before implementation.
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
            // Zero flow: neutronium-style hard insulation.
            DeltaKind::Mirror => 0,
            // Full flow from owner, but process_tile must not write the neighbor.
            // Returning `flow` here is correct once the caller is fixed.
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

/// Sparse map from cell-pair `(owner_idx, neighbor_idx)` to a contract.
/// The owner index is always the lower-xyz cell of the pair (owner-writes-positive scheme).
/// The neighbor index for `Void` and `Remote` pairs may be out of bounds or fictional.
pub type DeltaOverrides = std::collections::HashMap<(usize, usize), DeltaKind>;
