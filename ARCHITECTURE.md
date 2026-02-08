# Voxel Automata Architecture

## Overview

Rust cdylib + Luanti mod for 3D cellular automata visualization. Test-driven, bottom-up approach.

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

### Phase 6: Scale Up
- **Rust**: Replace `Vec<u8>` with bitpacked `Vec<u64>` (X-axis packed), add Rayon parallelism
- **Test**: Benchmark 256³ step time, verify correctness against naive on small grids

### Phase 7: Viewport + Change Tracking
- **Lua**: Only render viewport around player, use change tracking for incremental updates
- **Rust**: XOR grid for change detection, `va_get_changes_in_region()`
- **Test**: Walk around, viewport follows, incremental rendering works

## Data Structures

### Phase 3-5: Naive Grid
```rust
struct State {
    width: i16, height: i16, depth: i16,  // i16 matches Luanti's 2^16 world limit
    cells: Vec<u8>,  // 1 byte per cell: 0=dead, 1=alive
    generation: u64,
}
```

### Phase 6+: Bitpacked Grid
```rust
struct State {
    width: i16, height: i16, depth: i16,  // i16 matches Luanti's 2^16 world limit
    current: Vec<u64>,  // X-axis packed into u64 words
    next: Vec<u64>,     // double-buffered for stepping
    generation: u64,
    changes: Vec<u64>,  // XOR of current vs previous
}
```

- Layout: `word_idx = (z * height + y) * (width/64) + (x/64)`, `bit = x % 64`
- 1024³ = 128 MiB per grid, 256 MiB double-buffered

### Neighbor Counting (Phase 6+)

Moore neighborhood (26 neighbors). For bitpacked grids, use parallel bit-slice adder tree: decompose 26-neighbor sum into bit-planes, reduce via full-adder circuits on u64 words (processes 64 cells simultaneously).

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

## Performance Targets (Phase 6+)

- 256³ step: < 100ms
- 512³ step: < 500ms
- 1024³ step: < 5s (acceptable for slow visualization)
- 64³ VoxelManip full refresh: < 50ms
- Incremental update (<10k changes): < 10ms

## Future Extensions

- SIMD (AVX2) neighbor counting
- Thermal diffusion simulation
- Gas diffusion simulation
- Multi-rule layering
- GPU compute via wgpu/CUDA
