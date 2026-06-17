## Assistant

OK. I think I was wrong in my initial framing and you're correcting me. Let me lay out what I now understand.

The current "fused" flow computation — `(gradient * conductivity) / divisor` applied symmetrically to both cells in a pair — is just one *species* of delta contract. Call it the **modal diffusion** contract: both elements are local, same tick rate, same tick phase, same physical medium. It's the cheapest kind because it can be computed and applied in a single pass without any persistent state.

But the architecture describes a much broader taxonomy of contracts:

**By timing relationship:**
- **Synchronous** (modal): both sides tick together. Compute flow, apply, discard. What exists now.
- **Phase-offset**: both sides tick at the same rate but on different phases. The delta must persist for a fraction of a tick cycle — it's computed when side A ticks, buffered, applied when side B ticks.
- **Rate-mismatched**: fast side (60 Hz) accumulates deltas, slow side (1 Hz) drains them in bulk. The delta accumulates across many ticks of the fast side. This is Phase 9.
- **Asynchronous** (network): no shared clock at all. Delta persists in a local queue, gets picked up by the remote on next connection. The contract doesn't even know when the other side will resolve. This is the ClusterIO case.

**By topology:**
- **Voxel-to-voxel**: the modal case. One pair, uniform grid.
- **Subnode-to-voxel**: a machine's internal state has multiple ports, each connected to surrounding voxels via separate contracts. A furnace might have a heat-output port that pushes thermal energy into adjacent voxels, plus a material-input port that pulls matter from a different face. One-to-many with asymmetric flow rules.
- **Subnode-to-subnode**: two machine ports connected internally (e.g., heat exchanger). No grid involved.
- **Remote-to-local**: the delta's "other side" is on a different server. You can only write your half — the remote picks it up asynchronously.

**The key insight** is that all of these are the same abstraction: *a contract between two elements to exchange a conserved quantity*. What varies is:
1. How the flow is computed (gradient diffusion, fixed-rate injection, demand-driven pull)
2. When the flow resolves (same tick, buffered, accumulated, async)
3. What the elements are (voxel index, machine subnode, remote handle)

So the trait isn't about wrapping `compute_flow` — it's about defining the **contract lifecycle**: accumulate, buffer, resolve, and the guarantee that `delta_a + delta_b = 0` holds regardless of timing.

The modal diffusion contract is just the degenerate case where accumulate and resolve happen in the same call, so no persistent state is needed.

Does this match your thinking? And for 8B specifically — should the trait model this full lifecycle (accumulate/resolve as separate operations, with persistent state between them), with modal diffusion as the trivial implementation where both happen atomically? Or do you want to start narrower?

## User

elaborate the advantages and differences of each approach to the problem.

## Assistant



Let me think through this carefully against the codebase.

**Approach A: Full lifecycle trait from the start**

```rust
trait DeltaContract {
    fn accumulate(&mut self, source_a: u32, source_b: u32) -> i64;
    fn resolve(&mut self) -> (i64, i64);  // (delta_to_a, delta_to_b)
    fn is_resolved(&self) -> bool;
}
```

Advantages:
- Every future contract type (rate-mismatched, network, subnode) fits naturally. You never retrofit the abstraction.
- `PersistentDelta` (Phase 9) and `RemoteDelta` (Phase 11/ClusterIO) are just implementations that return `is_resolved() = false` until their slow side ticks or the network drains.
- `LoggingDelta` wraps any contract and observes its lifecycle — you can log a network delta's accumulation over 60 fast ticks before it resolves, which is the diagnostic tool you actually need for Phase 9+ debugging.
- The modal case implements `accumulate` + `resolve` in one call, `is_resolved` always returns true. Trivial.

Disadvantages:
- The hot path today processes ~2 million pairs per axis on a 128³ grid. Even if you keep the modal path as a non-trait fast path and only use the trait for override pairs, you need a mechanism to *select* which pairs get contracts. That's a lookup on every pair — a branch or a hash check in the inner loop.
- More concretely: `process_tile` in `incremental.rs` iterates cells and computes flows inline. To support per-pair contract overrides, you'd need either (a) a sparse map of `(idx_a, idx_b) -> Box<dyn DeltaContract>` checked per pair, or (b) a cell-level flag marking "this cell has non-modal contracts." Both add overhead to the modal path.
- You're designing the persistence model (when does a delta get serialized? who owns it?) before having a consumer. Phase 9 will tell you what persistence actually needs to look like — designing it now risks building the wrong thing.
- The `accumulate`/`resolve` split implies deltas live across ticks, which means they need storage, ownership, and lifecycle management (creation, resolution, garbage collection). That's a state management system, not just a trait.

**Approach B: Narrow extraction — composable flow computation**

```rust
trait FlowCompute {
    fn compute(&mut self, source_a: u32, source_b: u32, conductivity: i64, divisor: i64) -> i64;
}
```

Advantages:
- Directly replaces the `compute_flow` function that exists today. Drop-in.
- `FlowDelta` is the current formula. `LoggingDelta` wraps it and logs. Test passes immediately — you verify logged flows match the fused baseline on 128³ exactly as the ARCHITECTURE.md specifies.
- Zero overhead on the modal path — `FlowDelta::compute` inlines to the same code as today.
- Small, provable, shippable. You get the "can observe any pair's flow" capability that Track 8B asks for.

Disadvantages:
- Doesn't model persistence, timing, or element identity at all. When Phase 9 arrives with rate-mismatched regions, this trait doesn't help — you'd need to design the lifecycle abstraction then anyway.
- A subnode assembly doesn't compute flow via gradient diffusion at all. A machine port might inject at a fixed rate, or pull on demand. `FlowCompute` assumes gradient-driven diffusion, which is only one flow computation strategy.
- `LoggingDelta` under this approach can only log what happens within a single synchronous step. It can't observe the multi-tick accumulation of a persistent delta because that concept doesn't exist yet.
- Basically: you refactor the leaf, not the branch point. The next phase will require a different, larger abstraction that subsumes this one.

**Approach C: Element + Contract registry, modal as special case**

Don't trait-ify the flow computation. Instead, introduce the `ElementId` and `Delta` structures from the architecture doc, plus a registry that the stepping kernel consults:

```rust
enum ElementId {
    Voxel(usize),
    MachineSubnode(u32, u16),
}

struct Delta {
    a: ElementId,
    b: ElementId,
    quantity: i64,
    state: DeltaState,  // Pending, Accumulated, Resolved
}

struct DeltaRegistry {
    overrides: HashMap<(usize, usize), DeltaId>,  // voxel pair -> delta
    deltas: Vec<Delta>,
}
```

The stepping kernel checks `registry.overrides` for each pair. If present, it routes through the delta's lifecycle. If absent, it does the inline modal computation as today.

Advantages:
- Models the actual problem: deltas are *data*, not *behavior*. A network delta and a modal delta have the same struct, different lifecycle state. The contract between two elements is a row in a table, not an object with methods.
- The registry naturally supports the diagnostic need: iterate all deltas, filter by element, log flows. No wrapper pattern needed.
- Subnode assemblies work immediately: a machine registers N deltas connecting its ports to surrounding voxels. The registry holds them. The stepping kernel processes them when it encounters those voxel indices.
- Scales to network: a `RemoteElement` variant on `ElementId` means the delta's resolution is deferred. The registry just holds it in `Accumulated` state until the network layer drains it.
- The modal path stays fast: no override entry in the registry means the kernel does exactly what it does today. The registry check is a hash lookup that misses on 99.99% of pairs — branch predictor eats it.

Disadvantages:
- The `HashMap` lookup per pair is real overhead even if branch-predicted. On 128³ with 3 axes, that's ~6 million lookups per step. Even at 5ns each that's 30ms — significant against the ~100ms per-step budget.
- More realistically, you'd want a bitfield or per-cell flag to skip the lookup entirely for cells with no overrides. That's additional data structure to maintain.
- Bigger scope than 8B asks for. The registry, element IDs, delta lifecycle states — that's getting into Phase 9-10 territory. You'd be building infrastructure before the test that validates it.
- The `Delta` struct in the architecture doc has `accumulate_rate` and `resolve_rate` fields. You'd need to decide now whether those drive the lifecycle or whether they're advisory metadata. That's a design decision that benefits from Phase 9's concrete requirements.

**Approach D: Hybrid — lifecycle trait with data-oriented fast path**

Define the full lifecycle abstraction, but implement it as a data-oriented system rather than trait objects:

```rust
enum DeltaKind {
    /// Modal: compute and resolve in same tick. No persistent state.
    /// The stepping kernel handles this inline — never allocates a Delta struct.
    Modal,
    /// Persistent: accumulates across ticks, resolves on slow-side tick.
    Persistent { accumulated: i64, resolve_every: u32, ticks_since_resolve: u32 },
    /// Logged: wraps another kind, records flows for diagnostics.
    Logged { inner: Box<DeltaKind>, log: Vec<i64> },
}
```

The stepping kernel has two paths:
1. **Fast path**: no override for this pair → inline modal computation (identical to today's code, zero overhead).
2. **Slow path**: override exists → match on `DeltaKind`, dispatch accordingly.

The override map uses a compact representation — a sorted `Vec<(usize, usize, DeltaId)>` binary-searched, or a bitfield per cell marking "has non-modal contracts."

Advantages:
- Full lifecycle is modeled, but enum dispatch is cheaper than trait object virtual calls.
- The fast path is *literally* the same code as today — no branch, no lookup, no overhead for the 99.99% modal case. You only check for overrides when processing cells that are flagged.
- `Logged` variant directly satisfies the 8B test: wrap 2-3 pairs, step 128³, inspect the log vector.
- `Persistent` variant is exactly what Phase 9 needs. `RemoteElement` variant can be added later.
- Data-oriented: all deltas are in a flat `Vec<Delta>`, cache-friendly, serializable.

Disadvantages:
- The cell flagging mechanism (which cells have non-modal contracts?) needs design. A `BitVec` parallel to `cells` works but is another allocation to maintain.
- `DeltaKind::Logged { inner: Box<DeltaKind> }` is composition via nesting, not trait composition. Adding a new kind means adding an enum variant and match arms, not implementing a trait. That's fine for a small number of kinds but less extensible than traits if the taxonomy grows.
- Still a meaningful chunk of work for 8B. The bitfield, the override dispatch in the kernel, the enum — more than the architecture doc's "extract flow computation" phrasing suggests.

---

**Summary of the tradeoff axis:**

| | Modal overhead | Models persistence | Models topology | Scope for 8B |
|---|---|---|---|---|
| A: Full lifecycle trait | Medium (vtable + lookup) | Yes | Yes | Large |
| B: Narrow flow trait | Zero | No | No | Small |
| C: Registry + data | Low-medium (hash lookup) | Yes | Yes | Large |
| D: Hybrid enum + fast path | Zero (flagged cells only) | Yes | Partially | Medium |

The core tension: 8B's stated test is narrow (log some flows, verify against baseline), but the *purpose* of 8B is to build the foundation for Phases 9-11. Approach B passes the test but doesn't serve the purpose. Approaches A/C/D serve the purpose but are bigger.

What's your read on where the cut should be?

---

## Decision: Approach D

**Chosen: Hybrid enum (`DeltaKind`) + fast path.**

B was ruled out by the phase-change requirements for goal #1: boiling bubbles require buoyancy-driven advection, which has a fundamentally different computation shape than gradient diffusion — you can't model it by swapping in a different formula. B also can't coordinate two simultaneous contracts (heat + mass) on the same cell pair, which multi-field physics needs. D handles this by adding a `Buoyant` enum variant alongside `Modal` and `Logged`, with the `DeltaOverrides` map holding separate contracts per conserved quantity on the same pair.

D was preferred over A and C because the modal fast path (no override in the map → inline `compute_flow` verbatim, no lookup) keeps the 99.99% case at zero overhead, while A's vtable dispatch and C's per-pair `HashMap` lookup both add real cost at 6M pairs/step.

Note: the delta contract abstraction (Phase C in `process_tile`) is only half of what phase changes need. The Phase B hook (intra-cell state transition: substance identity, phase boundary threshold, latent heat accounting) needs its own design — a per-voxel `(substance_id, mass, thermal_energy)` struct and a material properties table. That's a separate design task before boiling can be demonstrated.

---

## Design session notes (2026-06-17): Delta contract architecture revision

### The "modal is common" assumption is wrong

The fast-path design assumed modal (uniform gradient diffusion) is the 99.99% case and override contracts are rare. This holds for empty/inert chunks (vacuum, solid rock far from players) but inverts in any loaded region:

- Player-built machines: dense override contracts on every active face
- Subnode assemblies (see below): many contracts per major voxel
- Multi-rate chunk boundaries: every face between adjacent chunks ticking at different rates
- Hull breaches, organisms, server boundaries

The performance argument changes: the branch and hash lookup that protect the modal fast path become overhead in exactly the chunks under simulation load. Chunk-level dispatch (not cell-level) is the right granularity — uniform chunks use the implicit 3-per-voxel tight loop, active chunks use an explicit flat contract list iterated without per-entry branching.

### Delta contracts as bound vectors / graph edges

A delta contract is an edge in a directed graph: it has a read-source (two cell values to diff), a write-target (where to apply the flow), and a flow computation. The 3-per-voxel spatial structure is a regular graph embedded in 3D. Nothing requires the graph to be regular.

The "neighbor" cell is the common case for the edge target, not a fixed assumption of the model. Its delta contracts express the simplest bound vector between two cells, but alternative or longer or out-of-bounds bound vectors are also possible inside of alternative delta contracts.

### Additive extra-contracts model

Rather than overriding/replacing existing spatial contracts, special voxels get **additional contracts** beyond their 3 spatial ones. The spatial contract on the affected face becomes Mirror (zero flow), and the extra contract carries the non-local or non-modal behavior. This is compositional:

- Space boundary face: Mirror on the spatial pair + Void extra contract (drain to bottomless vacuum)
- Portal face: Mirror on both spatial pairs + Remote extra contract linking the two mouths
- Multi-rate boundary: spatial contract stays but gains a Buffered accumulator alongside it

Hot path for an active chunk iterates a flat contract list with no per-entry kind dispatch. Each entry carries everything needed: read-source values (real or virtual), write targets (real or discard/network), and the flow computation variant. Normal voxels in a uniform chunk have no explicit contract list — the implicit 3-per-voxel loop handles them.

This maps directly onto the FEM framing from the optozorax report: portals are off-diagonal entries added to the stiffness matrix K, not modifications of existing entries.

### Void is directed (not symmetric)

From the optozorax FEM report: a void (single-mouth portal to nowhere) is **not** a degenerate symmetric portal pair. It is a directed absorbing sink that intentionally breaks stiffness-matrix symmetry. Symmetric coupling conserves the field; a one-way coupling or per-cell loss term is the discrete signature of a sink.

Void: owner loses mass (subtracted from `target[idx_a]`), spatial neighbor does NOT gain (`target[idx_b]` write suppressed). The virtual neighbor value for gradient computation is 0 (bottomless vacuum, permanently at floor).

### Remote is symmetric coupling across a topological discontinuity

From the optozorax FEM report: portal mouths are identified so the field is C¹-continuous across them. The gradient is computed from the two non-adjacent cell values with no distance correction ("surface irrelevance"). Flow is applied to both sides symmetrically. Conservation holds globally even though it appears violated locally on each server during network latency.

Remote is NOT void + later correction. It is symmetric coupling with async resolution.

### Open DeltaKind taxonomy

DeltaKinds are **not** being reduced to a minimal set. New kinds will be added as needed. Shared implementation lives in inline helper functions (`compute_flow` is already this pattern), not in trait unification. Current known kinds:

- `Modal` — uniform gradient diffusion, implicit fast path, no stored state
- `Mirror` — zero flow, insulating boundary (neutronium, map edges)
- `Logged` — diagnostic wrapper, records flow values per step
- `Void` — directed sink, virtual neighbor = bottomless vacuum (always 0), suppress neighbor write
- `Buffered` — cross-chunk multi-rate accumulator, same flow formula as Modal but accumulates across fast ticks and drains on slow-chunk tick
- `Remote` — cross-server symmetric coupling, async resolution, virtual neighbor = cached remote cell value
- `EntityExchange` — one end is a Luanti entity (not a voxel index), entity applies homeostasis resistance logic rather than passive diffusion; entity actively resists the gradient

### Subnode assemblies (future)

A player-built engine or machine fits inside a major voxel grid cell but contains many smaller internal nodes. The major voxel's face contracts are determined by the internal sub-simulation. Implementation: subnode assembly produces a set of effective face contracts that are inserted into the parent chunk's contract list. The parent simulation sees extra graph edges; it does not know or care about the internal structure.
