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

#### Phase 8: Persistent Deltas
- **Proves**: Deltas survive across ticks and correctly accumulate/drain at mismatched rates
- **Rust**: Refactor `field_step_fused` to produce persistent `Delta` structures instead of transient flow calculations. Delta connects two `ElementId` values, accumulates at source tick rate, resolves at drain tick rate.
- **Test (Rust)**: Two regions at different tick rates — fast side accumulates deltas, slow side drains on its tick. Total mass conserved across rate boundary. Single-rate case produces identical results to Phase 7 fused algorithm.

#### Phase 8: Non-Blocking Incremental Stepping
- **Proves**: Stepping work can be divided into time-budgeted tiles that process across multiple Luanti ticks without blocking frames
- **Rust**: Implement `StepController` with snapshot double-buffer architecture. Tiles (16³) process in Morton-order from immutable snapshot, accumulating flows into target buffer. `tick(budget_us)` does bounded work, returns true when step complete.
- **Key invariant**: Incremental stepper is bit-identical to `field_step_fused` — all reads from same generation-N snapshot, all writes to generation-N+1 buffer, commutative accumulation across tiles.
- **Test (Rust)**: Bit-identity test against fused baseline on 128³ for 4 generations. Incremental-across-ticks with small budget produces same result as blocking. Conservation, determinism, rotational symmetry all pass.
- **Implementation plan**: See `/home/kirrim/.claude/plans/tidy-dazzling-lagoon.md`

#### Phase 9: Hilbert Curve Indexing
- **Proves**: Hilbert-indexed field produces identical physics results with better cache performance
- **Rust**: Implement 3D Hilbert curve mapping. New `HilbertField` with same `field_set/get/step` interface but Hilbert-ordered memory layout. Neighbor lookup via Hilbert index arithmetic.
- **Test (Rust)**: Hilbert field produces identical diffusion results to row-major field on same input. Benchmark shows reduced cache misses on large grids (256^3+).

#### Phase 10: Nucleation Sites + Variable Tick Rates
- **Proves**: Regions tick at different rates while maintaining conservation across rate boundaries
- **Rust**: Tick-rate metadata per Hilbert curve segment. Nucleation site registration API. Tick scheduler that advances fast regions more often than slow ones. Persistent deltas bridge rate boundaries.
- **Test (Rust)**: Place nucleation site at center — surrounding region ticks at 60 Hz, distant region at 1 Hz. Mass conserved. Remove nucleation site — region relaxes to low tick rate. Machine nucleation site persists without player.

#### Phase 11: Heterogeneous Mesh Elements
- **Proves**: Machines with internal subnodes can exchange conserved quantities with voxel grid
- **Rust**: `ElementId` enum supporting both voxel coordinates and machine subnode IDs. One-to-many delta fan-out from machine internal state to surrounding voxels. Machine subnodes participate in same conservation framework.
- **Test (Rust)**: Machine subnode injects heat into surrounding voxels via deltas. Total energy conserved. Machine at fast/slow boundary — deltas accumulate correctly.

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

## Success Criteria

- Mass conservation holds across all boundary types (voxel-voxel, machine-voxel, fast-slow, world edge)
- Player walking between regions feels seamless (no pop-in, no LOD transitions)
- Machines work correctly regardless of surrounding tick rates
- Memory bandwidth scales sub-linearly with active voxel count
