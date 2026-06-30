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
(omitted)
**Approach B: Narrow extraction — composable flow computation**
(omitted)
**Approach C: Element + Contract registry, modal as special case**
(omitted)
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

table to help u:
DeltaKind taxonomy by axis:

| Kind | Physics | Temporal | Topology |
|---|---|---|---|
| Modal | normal | sync | local |
| Mirror | zero flow | sync | local |
| Buffered | normal | async | local |
| Void | sink | sync | non-local (virtual) |
| Remote | normal | async | non-local |
| Entity | extradimensional | async (Lua entity ticks at a different rate) | local (nearby Lua entity) |
| portal | normal | undefined | non-local but in the same grid (unlike remote) |
more deltakinds will be added
`Buffered` is Modal + temporal async. `Remote` is Buffered + topological non-local. They compose.


### Subnode assemblies (future)

A player-built engine or machine fits inside a major voxel grid cell but contains many smaller internal nodes. The major voxel's face contracts are determined by the internal sub-simulation. Implementation: subnode assembly produces a set of effective face contracts that are inserted into the parent chunk's contract list. The parent simulation sees extra graph edges; it does not know or care about the internal structure.
