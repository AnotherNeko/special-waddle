# Voxel Automata Architecture

## Overview

Rust cdylib + Luanti mod building toward a **billion-voxel ecological simulation** where everything runs at full fidelity always, but different regions tick at different wall-clock rates based on local physics complexity. The player never perceives simulation boundaries or level-of-detail changes.

Development is test-driven: each phase proves a specific capability through passing tests before moving to the next. Phases 1-7 are complete and form the proven foundation for the FEA architecture described below.

### Target Capabilities

| Downstream feature | What it needs from us |
|---|---|
| Weather with conservation of mass | Integer field diffusion via delta grids, mass-invariant by construction |
| Plants that consume/produce matter | Lua writes to Rust field mid-simulation, next step reflects it |
| Herbivore NPCs reading environment | Lua reads field values at arbitrary coordinates after step |
| Chemical spill spreading through water | Point-source diffusion, mass conservation, threshold-based node updates |
| Persistent world surviving restarts | Rust state serialization independent of Luanti nodes |
| Billion-voxel world at interactive rates | Hilbert indexing, variable tick rates, sub-linear memory bandwidth scaling |
| Machines running unattended | Nucleation-site-driven simulation independent of player proximity |

## Core Architecture: Finite Element Analysis on Voxels

### 1. Hilbert Curve Spatial Indexing

Maps 3D space to a 1D memory layout where physically adjacent voxels are adjacent in memory.

**Why it matters:**
- Better cache coherency — when a nucleation site (machine, player, reaction) needs a high tick rate, nearby voxels are already in cache
- Simulation rate becomes a property of local physics, not administrative chunks
- No hard chunk boundaries to create visible seams

**Current state:** Phase 6-7 uses row-major `z * height * width + y * width + x` indexing. Hilbert indexing replaces this as the spatial layout for all field data.

### 2. Nucleation Sites Drive Simulation Rate

Instead of uniform tick rates or chunk-based LOD, simulation rate emerges from local physics activity.

| Region state | Tick rate | Example |
|---|---|---|
| Equilibrium | ~1 Hz | Still lake, stable temperature field |
| Gradual change | ~10 Hz | Slow evaporation, seasonal shift |
| Active physics | ~60 Hz | Machine operation, player interaction, chemical reaction |

**Key behavior:**
- Tick rate gradient emerges naturally around activity centers
- Machine keeps running when player leaves — the machine is the nucleation site, not the player
- Player walking between regions feels seamless — no pop-in or LOD transitions
- Tick-rate metadata stored per spatial region along the Hilbert curve

### 3. Deltas as First-Class Persistent Structures

A delta is a **contract between two finite elements to exchange a conserved quantity**.

**Current state (Phase 6-7):** Deltas are transient — computed and applied within a single `field_step` call, then discarded. The fused algorithm (`field_step_fused`) computes all three axis flows from the original state and accumulates into a single buffer.

**Target architecture:** Deltas become persistent structures that outlive a single tick.

```rust
/// A persistent contract to exchange conserved quantity between two elements.
struct Delta {
    element_a: ElementId,  // source element (voxel, machine subnode, etc.)
    element_b: ElementId,  // destination element
    quantity: i64,         // accumulated transfer (positive = A→B)
}
```

**Delta properties:**
- Knows which two elements it connects
- Accumulates transfers at the rate of the faster element
- Resolves at the rate of the slower element
- Guarantees conservation by construction (Newton's 3rd law)

**Handles all boundary cases uniformly:**

| Boundary type | Delta behavior |
|---|---|
| Voxel ↔ voxel | Simple delta, both sides tick at same rate |
| Machine subnode ↔ voxel | One-to-many deltas from machine's internal state |
| Fast region ↔ slow region | Delta accumulates in buffer, drains when slow side ticks |
| World boundary | Element mirrors itself, flow = 0, automatic equilibrium |

### 4. Memory Bandwidth Optimization

Current bottleneck: ~5x memory traffic per step on a 512^3 grid saturates DDR5 bandwidth.

**Scaling strategy for billion voxels:**
- Hilbert indexing reduces cache misses (spatial locality)
- Variable tick rates mean less total computation (most voxels idle most of the time)
- Deltas only calculated for active boundaries
- Fused axis processing (proven in Phase 7) eliminates intermediate copies: 0.5 GB vs 2.5 GB DRAM traffic per step

## System

- **Hardware**: i5-12600K (16 threads), 32GB RAM, AVX2 support
- **Luanti**: 5.15.0 Flatpak with LuaJIT 2.1.1761786044
- **Rust**: 1.93.0
- **Repo**: `/home/kirrim/git/voxel-automata/`

## Communication: LuaJIT FFI

Luanti mod uses `minetest.request_insecure_environment()` to access `require("ffi")`, then loads the Rust `.so` via `ffi.load()`. Rust exports C ABI functions (`extern "C"`, `#[no_mangle]`, `#[repr(C)]`).

### Flatpak Integration

1. Symlink mod to `~/.var/app/org.luanti.luanti/.minetest/mods/voxel_automata`
2. Grant filesystem access: `flatpak override --user --filesystem=/home/kirrim/git/voxel-automata:ro org.luanti.luanti`
3. Add to trusted mods in `minetest.conf`: `secure.trusted_mods = voxel_automata`

## Development Workflow

### Commit Policy

**Do NOT commit code that hasn't been tested.** This is a test-driven project. Before committing:

1. **Rust changes**: Run `cargo test --release` and verify all tests pass
2. **Lua changes**: Test in Luanti using the provided chat commands or interactive features
3. **Integration changes**: Verify both Rust tests and Lua behavior work together

### Testing Methodology

- **Rust tests**: Use `cargo test` for unit testing Rust functions in isolation
- **Lua/integration tests**: Launch Luanti, join a world with the mod enabled, and manually verify behavior
  - Early phases (1-3): Check chat window using `/ca_test` command for test output
  - Later phases (4+): Use `/ca_render` or similar commands to visually inspect the rendered world

**Important**: Never commit a phase until you can manually verify the feature works in Luanti.

## Development Phases

### Proven Foundation (Phases 1-7)

These phases are complete with passing tests. They established the FFI bridge, grid abstractions, delta-based diffusion, and the fused stepping algorithm.

#### Phase 1: FFI Bridge Proof
- **Rust**: `extern "C" fn va_add(a: i32, b: i32) -> i32`
- **Lua**: Load library, call `va_add(2, 3)`, print result
- **Test**: Luanti prints "2 + 3 = 5" in chat

#### Phase 2: Opaque Handle Lifecycle
- **Rust**: `va_create() -> *mut State`, `va_destroy(*mut State)`, `va_get_generation(*const State) -> u64`
- **Lua**: Create handle on load, query generation, destroy on shutdown
- **Test**: No crash, generation = 0 printed

#### Phase 3: Small Grid + Step
- **Rust**: `va_create_grid(w,h,d)`, `va_set_cell(s,x,y,z,alive)`, `va_get_cell(s,x,y,z)`, `va_step(s)`
- Grid: `Vec<u8>` (one byte per cell), naive Moore (26-neighbor) counting
- Coordinates: `i16` for Luanti compatibility
- **Test (Rust)**: 8^3 grid patterns, boundary conditions, B4/S4 rule verification
- **Test (Lua)**: 16^3 grid, cross pattern, step, count alive cells

#### Phase 4: Visualize
- **Rust**: `va_extract_region(s, out_buf, min_xyz, max_xyz) -> u64`
- **Lua**: Node type registration, region rendering
- **Test**: Region extraction unit tests + visual inspection via `/ca_render`

#### Phase 5: Interactive Stepping
- **Lua**: `/ca_step`, `/ca_start`, `/ca_stop` commands, globalstep timer
- **Test**: Commands work, animation runs smoothly

#### Phase 6: Integer Field + Delta Diffusion
- **Proves**: Conservation-safe diffusion on integer grids through FFI
- **Rust**: `va_create_field(w,h,d)`, `va_field_set/get`, `va_field_step` with delta-based diffusion
- Delta approach: flow between adjacent pairs per axis, applied symmetrically. Newton's 3rd law guarantees conservation regardless of rounding.
- **Test (Rust)**: Point source 1,000,000 → step N times → grid sum unchanged. Symmetric spread.
- **Test (Lua)**: Create field via FFI, set point source, step, read neighbors

#### Phase 7: Fused Axis Processing + Algorithm Registry
- **Proves**: Fused simultaneous diffusion is rotationally symmetric and reduces DRAM traffic
- **Rust**: `field_step_fused` — all axes read from original state, accumulate into single buffer. Algorithm registry framework for systematic comparison testing.
- **Key result**: Sequential algorithm breaks rotational symmetry (known, documented). Fused algorithm is symmetric.
- **Test (Rust)**: Conservation on 128^3, determinism, rotational symmetry (2x2x2 cube), truth comparison against fused baseline, benchmarks at multiple grid sizes

### FEA Architecture (Phases 8+)

These phases build toward the full FEA simulation. Each phase generalizes the proven delta-grid diffusion into the persistent-delta, variable-tick-rate architecture.

#### Phase 8: Validate & Refactor Incremental Stepping for In-World Testing
- **Goal**: Establish that `StepController` (non-blocking incremental stepping) is production-ready by validating in Luanti and refactoring delta infrastructure for modularity
- **Current state**: `StepController` exists (Morton-ordered 16³ tiles, `tick(budget_us)` API, snapshot double-buffer) but untested in actual gameplay. Vacuum decay bug was fixed (commit 00dafbf) at the game level; Rust implementation is sound. Delta computation is transient (computed and discarded per-step).
- **Performance philosophy**: Performance > determinism. Small divergences (±25 per cell on 128³) between tile boundaries are acceptable. Parallelism (multiple tiles, future network async) is more valuable than bit-perfect reproducibility.

**Track A: In-World Performance Validation**
- **Lua test harness**: Create `/ca_perf size_x size_y size_z [tickrate_ms=1000]` command that measures real frame times during field stepping at commanded grid sizes
  - Display: FPS during step, time-budget utilization per tick, visual confirmation of no frame stalls
  - Ask player: "Does the world feel smooth? Any hitches during stepping?" → feedback loop
  - uses existing voxelmanip to render the entire field to nodes; it's important to also track lag during voxel operations
- **Success criteria**: 512³ stepping completes in < 75 ms per generation without frame drops below 20 FPS in 60-tick observation window
- **Blockage handling**: If performance regresses, profile with `cargo flamegraph` to find bottleneck (cache misses? tile sync overhead? Lua FFI overhead?)

**Track B: Delta Infrastructure Refactoring**
- **Goal**: Move toward persistent deltas (Phase 9+) by making delta contracts modular and readable
- **Current problem**: `field_step_fused` computes transient flows on-the-fly. The only way to observe them is the "bullshit-o-meter" debug test, which bypasses normal algorithm kernels.
- **Refactor**: Extract flow computation into composable `DeltaContract` abstractions that can be inserted into the stepping pipeline. Canonical form:
  ```rust
  trait DeltaCompute {
      fn step(&mut self, source_idx: usize, dest_idx: usize, source_val: u32, dest_val: u32) -> i64;
  }
  
  struct FlowDelta { /* standard fused diffusion */ }
  struct LoggingDelta { /* wraps FlowDelta, prints flows to log */ }
  struct PersistentDelta { /* accumulates across generations */ }
  ```
- **Benefit**: Can replace any single delta contract in the field with `LoggingDelta` to inspect real-time flows during stepping, supporting future network/clusterio design
- **Test (Rust)**: Wrap 2-3 random pairs in 128³ field with LoggingDelta, step, verify logged flows match reference fused algorithm's computed flows
- **No new FFI yet** — Rust-side only. Lua integration (Phase 9+) comes after architecture proves modular.

**Why both tracks matter**:
- Performance validation ensures stepping is usable in-world (track A is blocking for Phase 9)
- Delta modularity is the foundation for heterogeneous elements (machines, network clusterio, different tick rates) — can't build Phase 10+ without it (track B is blocking for Phase 10)

#### Phase 9: Persistent Deltas & Variable Tick Rates (Adjacent Regions)
- **Proves**: Two adjacent regions can tick at different rates while conserving mass across the boundary via persistent delta contracts
- **Goal**: Closest design target for FEA. Not full Hilbert reindexing yet (Phase 9.5), just two regions in a field with different tick rates.
- **Rust design**:
  - Refactor `field_step_fused` to emit persistent `Delta` structures instead of transient flows
  - `Delta` enum: `StandardFlow { source_idx, dest_idx, quantity }` (single-generation contract)
  - Fast region (e.g., 60 Hz): Computes deltas at every tick, accumulates flows
  - Slow region (e.g., 1 Hz): Only steps every N ticks, but drains accumulated deltas
  - **Network-ready design**: Delta can be `RemoteFlow { local_idx, remote_host, remote_idx }` for clusterio (data-in-transit, picked up next tick without knowing remote state)
- **Test (Rust)**: Two 64³ regions, boundary plane between them. Mark one as fast (step every tick), one as slow (step every 60th tick). Inject mass at center of fast region, observe gradient diffusing through boundary, verify total mass conserved
- **Test (Lua)**: In-world performance: spawn two adjacent 64³ fields at different tick rates, visually confirm no pop-in or jerky transitions at boundary

#### Phase 9.5: Hilbert Curve Indexing (Optional, Prior to Phase 10)
- **Proves**: Hilbert-indexed field produces identical physics results with better cache performance
- **Rust**: Implement 3D Hilbert curve mapping. New `HilbertField` with same `field_set/get/step` interface but Hilbert-ordered memory layout. Neighbor lookup via Hilbert index arithmetic.
- **Benefit**: Spatial locality for cache, enables nucleus-driven scheduling in Phase 10 (nearby voxels already hot)
- **Test (Rust)**: Hilbert field produces equivalent (not necessarily bit-identical, tolerance ±25) diffusion to row-major field on same input. Benchmark shows reduced cache misses on large grids (256^3+).

#### Phase 10: Nucleation Sites + Adaptive Tick Scheduling
- **Proves**: Simulation rate emerges from local physics activity (machines, reactions, player) rather than fixed chunks
- **Rust design**:
  - Nucleation site registry: machine center, chemical reaction, player position
  - Tick scheduler: Advance fast regions (60 Hz) more often than slow regions (1-10 Hz)
  - Persistent deltas accumulate at fast tick rate, resolve at slow tick rate
  - No hard chunk boundaries — tick rate gradient emerges smoothly
- **Test (Rust)**: Place nucleation site at grid center — measure tick rates at various distances (0 tiles away = 60 Hz, 10 tiles away = 30 Hz, 50 tiles away = 1 Hz). Remove nucleation site — region relaxes to idle tick rate. Machine nucleation site persists when player walks away.
- **Test (Lua)**: In-world: Spawn active machine → surrounding area becomes responsive. Leave area → ticks slow down. Return → speeds up. Measure frame times to ensure no correlation with background simulation complexity.

#### Phase 11: Heterogeneous Mesh Elements (Machines, Clusterio)
- **Proves**: Arbitrary "elements" (machines, network nodes, NPCs) can exchange conserved quantities via the same persistent delta framework that handles voxel-to-voxel flows
- **Rust design**:
  - `ElementId` enum: `Voxel(idx)`, `MachineSubnode(machine_id, port_id)`, `RemoteElement(remote_host, remote_id)` (clusterio)
  - One-to-many: Machine subnode(s) exchange with surrounding voxels via persistent deltas
  - Network-ready: `RemoteElement` delta persists in local queue, syncs to remote on next connection, no coupling to remote state
- **Real-world use case**: Clusterio with two factory servers sharing a border: Factory A's machine injects matter into border voxels, deltas accumulate, Factory B's stepping drains the deltas
- **Test (Rust)**: Machine subnode at fast tick rate (machine tick) injects heat into surrounding voxels at slow tick rate (field tick). Deltas accumulate, conserved mass confirmed.
- **Test (Lua)**: In-world: Machine placed at edge of viewport, verify it continues operating smoothly when player moves away (machine's own tick rate independent of viewport)

#### Phase 12: Lua Mid-Step Writes
- **Proves**: External agents (plants, NPCs, players) can modify simulation state between ticks
- **Rust**: `va_field_set()` works between `va_field_step()` calls without corruption
- **Test (Rust)**: Step, inject value, step again — injected value participates in diffusion, total mass adjusts correctly
- **Test (Lua)**: Player action writes to field, next step reflects it, value readable back

#### Phase 13: Threshold-Based Rendering
- **Proves**: Field values drive node appearance (biome, toxicity, temperature visualization)
- **Lua**: Extract `u32` region, map value ranges to node types or param2 colors
- **Test**: Set gradient in field, render, verify nodes reflect threshold bands

#### Phase 14: State Serialization
- **Proves**: Simulation survives server restart and crash recovery
- **Rust**: Serialize full simulation state including Hilbert-indexed fields, persistent deltas, nucleation site registry, and tick-rate metadata
- **Test (Rust)**: Round-trip serialize/deserialize — grid contents, deltas, tick rates, generation all identical
- **Test (Lua)**: Set pattern, save, destroy, reload, confirm state matches

#### Phase 15: Scale to Billion Voxels
- **Proves**: Architecture scales sub-linearly with active voxel count
- **Rust**: Benchmark memory bandwidth at 512^3 and 1024^3 with sparse nucleation sites. Verify that idle regions consume near-zero compute. Profile cache hit rates with Hilbert vs row-major at scale.
- **Test**: Memory bandwidth scales sub-linearly with active voxel count. 1024^3 field with 1% active voxels runs within DDR5 bandwidth budget.

## Data Structures

### Proven: Naive Grid (Phases 3-5)
```rust
struct State {
    width: i16, height: i16, depth: i16,
    cells: Vec<u8>,  // 1 byte per cell: 0=dead, 1=alive
    generation: u64,
}
```

### Proven: Integer Field (Phases 6-7)
```rust
struct Field {
    width: i16, height: i16, depth: i16,
    cells: Vec<u32>,       // integer value per cell (e.g. centigrams, microkelvin)
    generation: u64,
    diffusion_rate: u8,    // power-of-2 shift (e.g. 3 = divide by 8)
}
```

- No double-buffering — fused algorithm reads from original, accumulates into single output buffer
- Conservation by construction: `cell[i] -= flow; cell[i+1] += flow` (Newton's 3rd law)
- Diffusion rate as power-of-2 shift avoids integer division
- Memory: 256^3 x u32 = 64 MiB per field + 256^3 x u32 clone = 64 MiB working copy

### Target: FEA Simulation State (Phases 8+)
```rust
/// Element identifier — anything that participates in conservation exchange.
enum ElementId {
    Voxel(HilbertIndex),              // standard voxel on the grid
    MachineSubnode(MachineId, u16),   // internal node within a machine
}

/// Persistent contract to exchange conserved quantity between two elements.
struct Delta {
    element_a: ElementId,
    element_b: ElementId,
    quantity: i64,          // accumulated transfer (positive = A→B)
    accumulate_rate: u8,    // tick rate of faster element (Hz bucket)
    resolve_rate: u8,       // tick rate of slower element (Hz bucket)
}

/// Hilbert-indexed field with variable tick rates.
struct HilbertField {
    order: u8,                        // Hilbert curve order (10 = 1024^3)
    cells: Vec<u32>,                  // Hilbert-ordered cell values
    tick_rates: Vec<u8>,              // tick rate bucket per Hilbert segment
    deltas: Vec<Delta>,               // persistent inter-element contracts
    nucleation_sites: Vec<NucleationSite>,
    generation: u64,
    diffusion_rate: u8,
}

/// Source of simulation activity.
struct NucleationSite {
    element: ElementId,               // what drives the activity
    radius: u16,                      // influence radius in voxels
    tick_rate: u8,                    // requested tick rate (Hz bucket)
}
```

### Units

| Property | Type | Unit | Range | Resolution |
|---|---|---|---|---|
| Mass | `u32` | centigrams (per m^3) | 0 - 42,949 kg | 0.01 g |
| Temperature | `u32` | 1.5 uK | 0 - 6,442 K | 1.5 uK |

## 3D Game of Life Rules

Default rule: **B4/S4/5/M** (birth on 4 neighbors, survival on 4, 5 states for decay, Moore neighborhood)

- `B` = birth neighbor counts, `S` = survival neighbor counts
- Number after `/S` = state count (2 = binary, >2 = multi-state decay)
- `M` = Moore (26), `VN` = Von Neumann (6)

Other rules: **clouds** B13-14,17-19/S13-26/2/M, **amoeba** B9-26/S5-7,12-13,15/5/M, **coral** B5-8/S6-7,9,12/4/M

## Luanti Visualization

### Node Type
- One node: `voxel_automata:cell` with `paramtype2 = "color"`
- Palette texture: 16x16 = 256 colors (age-based gradient)
- param2 = cell age/state -> palette color index
- Dead cells = `air`

### Rendering Strategy

**Full refresh** (initial, viewport move):
- `va_extract_region()` -> flat u8 array
- VoxelManip bulk write (z,y,x order for cache efficiency)

**Incremental update** (per step):
- `va_get_changes_in_region()` -> list of changed cells
- If few changes: `minetest.swap_node()` per change
- If many changes: fall back to full refresh

### Viewport

The full simulation can be up to 1024^3 but only a movable window (default 64^3) is rendered as Luanti nodes. Viewport re-centers when player moves >1/3 width from center.

## Performance Targets

| Operation | Target |
|---|---|
| 256^3 u32 field step (fused, serial) | < 200ms |
| 256^3 u32 field step (fused, parallel) | < 100ms |
| 64^3 VoxelManip full refresh | < 50ms |
| 1024^3 field, 1% active, per-tick | Within DDR5 bandwidth budget |
| Memory bandwidth scaling | Sub-linear with active voxel count |

## Persistence & Data Ownership

### Principle: Rust is Source of Truth

Luanti world nodes are a **view layer only**. Rust state is the authority and must persist itself. Luanti nodes display the viewport slice using quantized values.

**Why not Luanti nodes as source of truth?** For dense field simulations (thermal, weather), nearly every cell changes every tick. The simulation grid may be larger than the rendered viewport. Quantization means Luanti nodes can't represent the full-precision state.

### Bidirectional Sync

- **Nodes -> Rust**: External mod actions (node_dig, boring machines) propagate to Rust field via FFI
- **Rust -> Nodes**: Only when field value crosses display thresholds (rate-limited to ~80k nodes/sec)

### Quantization Example

Water field:
- Rust: u32 centigrams (0 - 42,949 kg resolution)
- Luanti: 8 water levels (0 = air, 1-7 = increasing, 8 = source)
- Lake evaporating: 15,000 -> 14,999 -> 14,998 cg in Rust, node stays level 1 until below 12,500. **Minimal node updates despite continuous physics.**

### Persistence Strategy

- Rust serializes full state to disk (mod_storage or world directory file)
- Save on: server shutdown, autosave hooks, periodic intervals
- Load on: server startup (before first tick)
- On crash: rolls back to last save (at most a few ticks lost)
- Luanti world DB stores quantized nodes for mod compatibility + crash recovery

### Mod Compatibility

Third-party mods see standard Luanti nodes, interact normally. Lua wrapper ensures node changes propagate to Rust field.

## Design Philosophy: Performance Over Determinism

### Why Small Divergences Are Acceptable

The voxel automata simulation is designed for **ecological realism and player experience**, not multiplayer sync or bit-perfect determinism (unlike Factorio). Key principles:

1. **Performance > Determinism**: Non-blocking tiling (Phase 8) with tile-boundary remainder accumulation may produce ±25 per-cell divergences from sequential stepping. This is **acceptable** because:
   - Players never see tile boundaries (no LOD pop-in)
   - Stochastic rounding creates realistic small-scale fluctuations (±1-2 per generation)
   - Parallelism enables 5× speedup: worth the cosmetic variance

2. **Async Network-Ready**: Future clusterio extension uses asynchronous delta contracts where two servers exchange mass/heat without strong coupling:
   - Server A injects, Server B drains — no guarantee of perfect tick synchronization
   - Delta persists in transit queue, resolved on next connection window
   - Players expect environmental variation (weather, chaos) anyway — perfect determinism would feel artificial

3. **Master/Client Desync Philosophy**: If Luanti multiplay is added later:
   - Master server is authoritative (runs full simulation)
   - Clients get quantized view updates (same as single-player viewport)
   - Clients never try to "fix" physics divergence — just render what master sends
   - This mirrors weather systems in multiplayer games (server's weather is ground truth)

### Consequence: In-World Testing Required

Because small code changes can affect performance significantly (cache misses, tile sync overhead), **every Phase 8+ change must be validated in Luanti with `/ca_perf` or similar instrumentation**. Rust benchmarks alone are insufficient; real gameplay frame times matter more.

## Success Criteria

- Mass conservation holds across all boundary types (voxel-voxel, machine-voxel, fast-slow, world edge)
- Player walking between regions feels seamless (no pop-in, no LOD transitions)
- Machines work correctly regardless of surrounding tick rates
- Memory bandwidth scales sub-linearly with active voxel count
