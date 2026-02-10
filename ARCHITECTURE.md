# Voxel Automata Architecture

## Overview

Rust cdylib + Luanti mod proving the capabilities needed for complex voxel-world simulations (weather, ecology, chemistry). This project is a **test harness, not the simulation itself** — each phase proves a specific capability through passing tests. Upstream takes inspiration from this source to build production systems.

### Target Capabilities (demanded by upstream)

These are the downstream features that motivate what we prove:

| Downstream feature | What it needs from us |
|---|---|
| Weather with conservation of mass | Integer field diffusion via delta grids, mass-invariant by construction |
| Plants that consume/produce matter | Lua writes to Rust field mid-simulation, next step reflects it |
| Herbivore NPCs reading environment | Lua reads field values at arbitrary coordinates after step |
| Chemical spill spreading through water | Point-source diffusion, mass conservation, threshold-based node updates |
| Persistent world surviving restarts | Rust state serialization independent of Luanti nodes |

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

Attempting to commit untested code wastes time and breaks the development flow. Test first, then commit.

## Development Phases (Test-Driven)

Each phase has a passing test before moving to the next.

### Testing Methodology

- **Rust tests**: Use `cargo test` for unit testing Rust functions in isolation
- **Lua/integration tests**: Launch Luanti, join a world with the mod enabled, and manually verify behavior
  - Early phases (1-3): Check chat window using `/ca_test` command for test output
  - Later phases (4+): Use `/ca_render` or similar commands to visually inspect the rendered world
  - Rationale: Manual testing in Luanti is the most practical approach for validating Lua code and FFI integration

**Important**: Never commit a phase until you can manually verify the feature works in Luanti.

### Phase 1: FFI Bridge Proof
- **Rust**: `extern "C" fn va_add(a: i32, b: i32) -> i32`
- **Lua**: Load library, call `va_add(2, 3)`, print result
- **Test**: Luanti prints "2 + 3 = 5" in chat

### Phase 2: Opaque Handle Lifecycle
- **Rust**: `va_create() -> *mut State`, `va_destroy(*mut State)`, `va_get_generation(*const State) -> u64`
- **Lua**: Create handle on load, query generation, destroy on shutdown
- **Test**: No crash, generation = 0 printed

### Phase 3: Small Grid + Step
- **Rust**: `va_create_grid(w,h,d)`, `va_set_cell(s,x,y,z,alive)`, `va_get_cell(s,x,y,z)`, `va_step(s)`
- Grid: `Vec<u8>` (one byte per cell), naive Moore (26-neighbor) counting
- Coordinates: `i16` (saves memory for Luanti compatibility)
- **Test (Rust)**: Unit tests — 8³ grid patterns, boundary conditions, B4/S4 rule verification (all passing)
- **Test (Lua)**: Create 16³ grid, set cross pattern (5 cells), step, count alive cells (all passing)

### Phase 4: Visualize
- **Rust**: `va_extract_region(s, out_buf, min_xyz, max_xyz) -> u64` — extracts rectangular region into flat buffer
- **Lua**: Register node type, render regions with direct node placement (VoxelManip for Phase 6+ optimization)
- **Test**: 
  - Rust: Unit tests for region extraction (basic, full grid, empty, out-of-bounds, null checks)
  - Lua: Run `/ca_test` in Luanti to verify FFI and automation logic; run `/ca_render` to visually inspect 16³ cross pattern at player location

### Phase 5: Interactive Stepping
- **Lua**: `/ca_step`, `/ca_start`, `/ca_stop` commands, globalstep with timer
- **Test**: Commands work, animation runs smoothly

### Phase 6: Integer Field + Delta Diffusion
- **Proves**: Conservation-safe diffusion on integer grids through FFI
- **Rust**: `va_create_field(w,h,d) -> *mut Field`, `va_field_set/get(f,x,y,z,value: u32)`, `va_field_step(f)` with delta-based diffusion
- Delta approach: compute flow between each adjacent cell pair per axis, apply symmetrically (`cell[x] -= delta; cell[x+1] += delta`). Newton's third law guarantees conservation regardless of rounding.
- **Test (Rust)**: Set one cell to 1,000,000, step N times, verify grid sum is unchanged (exact conservation). Verify spread pattern is symmetric.
- **Test (Lua)**: Create field via FFI, set point source, step, read neighbors, confirm values changed

### Phase 7: Axis-Parallel Threading
- **Proves**: Delta grids per axis can be computed independently in parallel
- **Rust**: Separate delta grids for dx, dy, dz. Each axis computed by a separate Rayon task. dy includes gravity bias term.
- **Test (Rust)**: Compare parallel result against serial result on same input — must be identical. Verify gravity causes downward net flow in dy.

### Phase 8: Lua Mid-Step Writes
- **Proves**: External agents (plants, NPCs, players) can modify simulation state between ticks
- **Rust**: `va_field_set()` works between `va_field_step()` calls without corruption
- **Test (Rust)**: Step, inject value, step again, verify injected value participates in diffusion and total mass adjusts correctly
- **Test (Lua)**: Player action writes to field, next step reflects it, value readable back

### Phase 9: Threshold-Based Rendering
- **Proves**: Field values drive node appearance (biome, toxicity, temperature visualization)
- **Lua**: Extract `u32` region, map value ranges to different node types or param2 colors
- **Test**: Set gradient in field, render, verify nodes reflect threshold bands (e.g. 0 = air, 1-5000 = water, >5000 = ice)

### Phase 10: State Serialization
- **Proves**: Simulation survives server restart and crash recovery
- **Rust**: `va_serialize(s, buf) -> size`, `va_deserialize(buf, size) -> *mut State`
- **Lua**: Save to world directory on shutdown, reload on startup, verify state matches
- **Test (Rust)**: Round-trip serialize/deserialize, verify grid contents and generation identical
- **Test (Lua)**: Set pattern, save, destroy, reload, confirm cells match

### Phase 11: Scale Up
- **Proves**: Performance at world-relevant grid sizes
- **Rust**: Rayon parallelism for field stepping, optional bitpacking for binary layers
- **Test**: Benchmark 256³ u32 field step time, verify correctness against serial on small grids

## Data Structures

### Phase 3-5: Naive Grid
```rust
struct State {
    width: i16, height: i16, depth: i16,  // i16 matches Luanti's 2^16 world limit
    cells: Vec<u8>,  // 1 byte per cell: 0=dead, 1=alive
    generation: u64,
}
```

### Phase 6+: Integer Field with Delta Grids
```rust
struct Field {
    width: i16, height: i16, depth: i16,
    cells: Vec<u32>,       // integer value per cell (e.g. centigrams, microkelvin)
    generation: u64,
    diffusion_rate: u8,    // power-of-2 shift (e.g. 3 = divide by 8)
}

// Computed per-step, one per axis, then applied to cells
struct DeltaGrid {
    // Flow between adjacent cell pairs along one axis
    // delta[i] = flow from cell[i] to cell[i+1] (signed)
    deltas: Vec<i32>,
}
```

- No double-buffering needed — delta grids are computed from current state, then applied in-place
- Conservation by construction: `cell[i] -= delta; cell[i+1] += delta` (Newton's third law)
- One DeltaGrid per axis (dx, dy, dz), computed independently by separate threads
- dy includes gravity bias term for pressure/buoyancy effects
- Diffusion rate as power-of-2 shift avoids integer division
- Memory: 256³ × u32 = 64 MiB per field + 3 × 256³ × i32 = 192 MiB delta grids (transient)

### Units

| Property | Type | Unit | Range | Resolution |
|---|---|---|---|---|
| Mass | `u32` | centigrams (per m³) | 0 – 42,949 kg | 0.01 g |
| Temperature | `u32` | 1.5 µK | 0 – 6,442 K | 1.5 µK |

### Phase 11: Bitpacked Grid (optional, for binary layers)
```rust
struct BitGrid {
    width: i16, height: i16, depth: i16,
    current: Vec<u64>,  // X-axis packed into u64 words
    next: Vec<u64>,
    generation: u64,
}
```

- Layout: `word_idx = (z * height + y) * (width/64) + (x/64)`, `bit = x % 64`
- Useful for binary state layers (alive/dead, wet/dry) where bitpacking + SIMD gives large speedups

## 3D Game of Life Rules

Default rule: **B4/S4/5/M** (birth on 4 neighbors, survival on 4, 5 states for decay, Moore neighborhood)

- `B` = birth neighbor counts
- `S` = survival neighbor counts  
- Number after `/S` = number of states (2 = binary, >2 = multi-state decay)
- `M` = Moore (26), `VN` = Von Neumann (6)

Other interesting rules:
- **clouds**: B13-14,17-19/S13-26/2/M (organic cave structures)
- **amoeba**: B9-26/S5-7,12-13,15/5/M (flowing growth)
- **coral**: B5-8/S6-7,9,12/4/M (branching)

## Luanti Visualization

### Node Type
- One node: `voxel_automata:cell` with `paramtype2 = "color"`
- Palette texture: 16×16 = 256 colors (age-based gradient)
- param2 = cell age/state → palette color index
- Dead cells = `air`

### Rendering Strategy

**Full refresh** (initial, viewport move):
- `va_extract_region()` → flat u8 array
- VoxelManip bulk write (z,y,x order for cache efficiency)

**Incremental update** (per step):
- `va_get_changes_in_region()` → list of changed cells
- If few changes: `minetest.swap_node()` per change
- If many changes: fall back to full refresh

### Viewport (Phase 7+)

The full simulation can be up to 1024³ but only a movable window (default 64³) is rendered as Luanti nodes. Viewport re-centers when player moves >1/3 width from center.

## Performance Targets (Phase 7+)

- 256³ u32 field step (3-axis delta, serial): < 200ms
- 256³ u32 field step (3-axis delta, parallel): < 100ms
- 64³ VoxelManip full refresh: < 50ms

## Persistence & Data Ownership

### Problem

Rust FFI state is volatile — it's destroyed on server stop and lost on crash. The simulation needs to survive server restarts and unexpected shutdowns.

### Decision: Rust High-Resolution State + Quantized Node Sync

Luanti world nodes are a **view layer only** — they display the viewport slice but are not the source of truth. Rust state is the authority and must persist itself.
**Rust field is source of truth** (high resolution u32 values), **Luanti nodes are quantized view** (low resolution display).

**Why not use Luanti nodes as source of truth?**

For sparse automata (Game of Life), syncing nodes back via `va_import_region` on startup would work. But for dense field simulations (thermal diffusion, weather), nearly every cell changes every tick. The simulation grid may also be larger than the rendered viewport. Storing the full simulation in Luanti nodes would mean either:
- Materializing the entire grid as world nodes (wasteful, slow VoxelManip writes)
- Losing non-visible state outside the viewport

**Bidirectional sync:**
- **Nodes → Rust**: When external mods modify nodes (`minetest.node_dig()`, boring machines, etc.), Lua updates Rust field via FFI
- **Rust → Nodes**: Only when field value crosses display thresholds (rate-limited to ~80k nodes/sec)

**Persistence strategy:**
- Rust grid state serializes to disk via `mod_storage` or a raw file in the world directory
- Save on: server shutdown, Luanti autosave hooks, periodic intervals
- Load on: server startup (before first tick)
- On crash: state rolls back to last save (acceptable — at most a few ticks lost)

**Why quantization enables full-grid sync:**

Water example:
- Rust field: u32 centigrams (0 – 42,949 kg resolution)
- Luanti node: 8 water levels (0 = air, 1-7 = increasing fullness, 8 = source)
- Threshold mapping: 0 cg → air, 1-12,500 cg → level 1, 12,501-25,000 cg → level 2, etc.

Lake slowly evaporating: 15,000 → 14,999 → 14,998 cg in Rust, but node stays level 1 until drop below 12,500. **Minimal node updates despite continuous physics.**

Boring machine digs lake wall: Node removed → Lua zeros Rust cell → diffusion floods tunnel → nodes update only when thresholds crossed.

**Performance:**
- Full grid materialized as nodes (128³ to 256³ target)
- Simulation tick rate limited by 80k nodes/sec write budget
- Node updates sparse due to quantization (most ticks update <1% of grid)

**Persistence:**
- Rust serializes to mod_storage (high-resolution state preserved)
- Luanti world DB stores quantized nodes (mod compatibility + crash recovery)
- On startup: Restore Rust state from mod_storage, sync nodes if needed

**Mod compatibility:**
Third-party mods see standard Luanti nodes, can interact normally. Lua wrapper ensures node changes propagate to Rust field.

### Simulation Types

| Type | Example | Cell changes/tick | Sync strategy |
|---|---|---|---|
| Sparse | Game of Life | Few | Incremental (changed cells only) |
| Dense | Heat diffusion, weather | Most/all | Full viewport refresh each tick |

For both types, Rust holds the full grid in memory, computes in-place, and only exports the viewport region to Luanti nodes for display.

## Future Extensions

- SIMD (AVX2) neighbor counting
- Thermal diffusion simulation
- Gas diffusion simulation
- Multi-rule layering
- GPU compute via wgpu/CUDA
