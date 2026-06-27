//! Delta contract abstraction for cell-pair flow computation.
//!
//! Two separate enums govern the two processing paths:
//!
//! ## `DeltaKind` — spatial pair overrides (tile pass)
//!
//! Overrides the behavior of a pair the tile pass encounters during its
//! 3-per-voxel sweep: (x,y,z)↔(x+1,y,z), etc. Both endpoints are real
//! in-grid cells at the time the override fires. Stored in `DeltaOverrides`.
//!
//! - `Modal`    — normal gradient diffusion. Explicit override that behaves
//!                identically to the implicit fast path; useful for wrapping.
//! - `Logged`   — Modal + records each flow value for diagnostics.
//! - `Mirror`   — zero flow, perfect insulation (neutronium, map edges).
//! - `Buffered` — accumulates flow across fast ticks, drains on the Nth tick.
//!                Models the interface between two simulation regions ticking
//!                at different rates or different phases.
//!
//! ## `ContractKind` — non-spatial extra edges (ContractList post-pass)
//!
//! Extra graph edges that the tile pass never encounters. At least one endpoint
//! is not the implicit spatial neighbor: it may be non-adjacent, virtual, or
//! external. Stored in `ContractList`.
//!
//! - `Portal`  — symmetric coupling between two non-adjacent in-grid cells
//!               (e.g. opposite faces of a torus). Both endpoints are local
//!               indices but are not spatial neighbors.
//! - `Void`    — directed sink. One real endpoint; other side is bottomless
//!               vacuum (virtual 0). Mass is subtracted from the owner and
//!               destroyed. `consumed` accumulates total flow for diagnostics.
//!               Related: `Infinity` (TODO) — like Void but the virtual side
//!               holds a configurable value (Factorio infinity pipe analogue).
//! - `Remote`  — symmetric coupling across servers (Clusterio). Like Portal
//!               but one endpoint lives on a different game server; resolved
//!               async via ghost-cell sync.
//! - `Entity`  — one endpoint is a Luanti entity (Lua object reference). The
//!               entity applies homeostasis resistance rather than passive
//!               diffusion; it ticks at the Lua entity rate.

/// Spatial pair override. Applied by the tile pass when it encounters the pair.
/// Both endpoints are real in-grid cells.
pub enum NeighborKind {
    /// Gradient diffusion identical to the inline fast path.
    Modal,
    /// Same as Modal but records each flow value for diagnostics.
    Logged { log: Vec<i64> },
    /// Zero flow — perfect insulation regardless of gradient.
    Mirror,
    /// Accumulates flow across `drain_every` fast ticks, then drains in bulk.
    /// Returns 0 each non-drain tick so no mass moves until the drain fires.
    Buffered {
        accumulated: i64,
        drain_every: u32,
        ticks: u32,
    },
}

impl NeighborKind {
    /// Compute (and optionally record/accumulate) the flow for this pair.
    /// `compute_fn` is the modal formula; all variants call it for comparability.
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
            NeighborKind::Modal => flow,
            NeighborKind::Logged { log } => {
                log.push(flow);
                flow
            }
            NeighborKind::Mirror => 0,
            NeighborKind::Buffered {
                accumulated,
                drain_every,
                ticks,
            } => {
                *accumulated += flow;
                *ticks += 1;
                if *ticks >= *drain_every {
                    let drained = *accumulated;
                    *accumulated = 0;
                    *ticks = 0;
                    drained
                } else {
                    0
                }
            }
        }
    }

    pub fn new_logged() -> Self {
        NeighborKind::Logged { log: Vec::new() }
    }

    pub fn log(&self) -> Option<&[i64]> {
        match self {
            NeighborKind::Logged { log } => Some(log),
            _ => None,
        }
    }
}

/// Sparse map from spatial cell-pair `(owner_idx, neighbor_idx)` to a DeltaKind override.
/// The owner is the lower-xyz cell (owner-writes-positive scheme).
pub type NeighborOverrides = std::collections::HashMap<(usize, usize), NeighborKind>;

// ---------------------------------------------------------------------------
// ContractList — non-spatial extra edges
// ---------------------------------------------------------------------------
//
// A contract is a graph edge beyond the 3-per-voxel implicit spatial loop.
// Processed after the tile pass, reading from the frozen source snapshot.
//
// Memory layout: Contract entries are fixed-size for cache-friendly iteration.
// `src_b` and `dst_b` interpretation is kind-driven:
//
//   Portal          → local voxel index (non-adjacent but in same grid)
//   Void            → unused (B-side read is virtual 0; B-side write discarded)
//   Remote / Entity → aux_idx into the appropriate side table below

/// Side-table entry for Remote contracts.
pub struct RemoteEndpoint {
    pub server_id: u32,
    pub remote_voxel: u32,
    /// Last-known value of the remote cell (ghost cell, updated each network sync).
    pub cached_value: u32,
    /// Flow accumulated since last network sync.
    pub accumulated: i64,
}

/// Side-table entry for Entity contracts.
pub struct EntityHandle {
    /// Opaque Lua registry reference; resolved by the FFI layer, not Rust.
    pub lua_ref: u64,
}

/// A single non-spatial graph edge.
/// All fields are fixed-size; `src_b`/`dst_b` meaning is kind-driven.
pub struct Contract {
    pub src_a: u32,
    pub src_b: u32,
    pub dst_a: u32,
    pub dst_b: u32,
    pub kind: ContractKind,
}

/// Non-spatial extra edge kind. Processed by the ContractList post-pass.
pub enum ContractKind {
    /// Symmetric coupling between two non-adjacent in-grid cells.
    Portal,
    /// Directed sink to bottomless vacuum. `consumed` tracks total flow drained.
    Void { consumed: i64 },
    /// Cross-server symmetric coupling. src_b/dst_b index into `remote_endpoints`.
    Remote,
    /// One endpoint is a Luanti entity. src_b/dst_b index into `entity_handles`.
    Entity,
}

/// Flat list of non-spatial contracts for a field region.
pub struct ContractList {
    pub contracts: Vec<Contract>,
    /// Side table for Remote contracts.
    pub remote_endpoints: Vec<RemoteEndpoint>,
    /// Side table for Entity contracts.
    pub entity_handles: Vec<EntityHandle>,
}

impl ContractList {
    pub fn new() -> Self {
        ContractList {
            contracts: Vec::new(),
            remote_endpoints: Vec::new(),
            entity_handles: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }
}
