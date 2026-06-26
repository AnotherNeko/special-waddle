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
    /// `consumed` accumulates total flow drained, analogous to `Logged::log`.
    /// Use for cells adjacent to out-of-bounds vacuum (open space, hull breach).
    Void { consumed: i64 },
    /// Symmetric coupling to a non-adjacent cell in the same grid (portal mouth).
    /// Flow is computed and applied to both sides just like Modal; the kernel
    /// uses the two endpoint indices directly rather than the implicit spatial neighbor.
    Portal,
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
            // Zero flow: neutronium-style perfect insulation.
            DeltaKind::Mirror => 0,
            DeltaKind::Void { consumed } => {
                *consumed += flow;
                flow
            }
            // Symmetric coupling to a non-adjacent cell; same as Modal from apply()'s perspective.
            // The kernel must use the correct endpoint indices rather than the implicit neighbor.
            DeltaKind::Portal => flow,
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

// ---------------------------------------------------------------------------
// Flat contract list (replaces DeltaOverrides long-term)
// ---------------------------------------------------------------------------
//
// A contract is a graph edge: two endpoints, a flow computation, and write
// targets. The 3-per-voxel spatial loop is the implicit regular graph; this
// list holds only the extra edges that fall outside it (mirror faces, portals,
// hull breaches, entity exchanges, cross-server links).
//
// Memory layout by topology:
//
//   Local (Modal, Mirror, Buffered, Void, Portal): both endpoints are u32
//   voxel indices in this field. Void's B-side read is virtual const-0
//   (bottomless vacuum); its B-side write is discarded. Portal's endpoints
//   are also plain u32 indices — topologically non-adjacent but in the same
//   address space, so no extra storage.
//
//   Non-local (Remote, Entity): one endpoint requires extra data that doesn't
//   fit in a u32. Remote needs a server ID, a remote voxel index, a cached
//   ghost value, and an accumulator (~20 bytes). Entity needs an opaque Lua
//   object reference (8 bytes). These store a u32 `aux_idx` in src_b/dst_b
//   pointing into the appropriate side table (remote_endpoints or
//   entity_handles). The Contract entry itself stays fixed-size for every
//   kind, keeping the hot loop uniform-width and cache-friendly.
//
// ContractKind drives interpretation of src_b/dst_b:
//   Modal / Mirror / Portal / Buffered  →  local voxel index
//   Void / Infinity                     →  src_b unused (read 0); dst_b unused (discard)
//   Remote / Entity                     →  aux_idx into side table

/* TODO: `Infinity` is like Void but
the contract stores a snapshot of a tile which can source or sink flow but
resets to a value configured in the in-game creative mode gui (instead of
always being vacuum). Like the infinity pipe in Factorio 1.1 */

/// Side-table entry for Remote contracts.
pub struct RemoteEndpoint {
    pub server_id: u32, //might become an ipv6 address
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

/// A single non-spatial graph edge in the contract list.
///
/// All fields are fixed-size. `src_b` and `dst_b` are plain `u32` whose
/// meaning is kind-driven — see the module comment above.
pub struct Contract {
    pub src_a: u32,
    pub src_b: u32,
    pub dst_a: u32,
    pub dst_b: u32,
    pub kind: ContractKind,
}

pub enum ContractKind {
    /// Normal gradient diffusion, symmetric. Equivalent to the implicit spatial loop.
    Modal,
    /// Zero flow, insulating boundary (neutronium, map edges).
    Mirror,
    /// Symmetric coupling to a non-adjacent cell in the same grid (portal mouth).
    Portal,
    /// Directed mass sink. B-side read is virtual 0; B-side write is discarded.
    Void,
    /// Same formula as Modal but accumulates across fast ticks, drains on slow-chunk tick.
    /// `drain_every`: how many fast ticks between drains. `ticks`: counter since last drain.
    Buffered {
        accumulated: i64,
        drain_every: u32,
        ticks: u32,
    },
    /// Cross-server symmetric coupling. src_b/dst_b index into `remote_endpoints`.
    /// Async: flow accumulates until the next network sync.
    Remote,
    /// One endpoint is a Luanti entity. src_b/dst_b index into `entity_handles`.
    /// Entity applies homeostasis resistance rather than passive diffusion;
    /// it ticks at the Lua entity rate, not the voxel rate.
    Entity,
}

/// Flat list of non-spatial contracts for a field region.
pub struct ContractList {
    pub contracts: Vec<Contract>,
    /// Side table for Remote contracts; indexed by Contract::src_b / dst_b.
    pub remote_endpoints: Vec<RemoteEndpoint>,
    /// Side table for Entity contracts; indexed by Contract::src_b / dst_b.
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
