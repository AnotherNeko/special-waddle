# Phase 9c Dev Notes: KD-Tree Cadence Assignment + Luanti Visualization

## Goal

Prove that a `RefinementAnchor` (here: a Mesecons-powered SeamPlane node) can drive the
KD-tree to build a cadence gradient in-world. Visual inspection replaces unit tests as the
primary proof: the player places SeamPlane nodes, energizes them with Mese, and watches
the cadence zone overlay update in real time alongside the diffusing mass field.

---

## Codebase Quick Reference

```
special-waddle/
  rust/src/
    automaton/
      cadence.rs        -- CadenceTree, Gaaabb, SeamPlane, SyncStatus, Cadence
      incremental.rs    -- StepController (owns cadence_partition + global_tick)
    ffi/
      incremental.rs    -- existing FFI for StepController (va_sc_*)
      cadence.rs        -- NEW: cadence FFI functions (see below)
    lib.rs              -- pub use re-exports; add new cadence FFI here
  mod/
    init.lua            -- ffi.cdef block + module load order
    animation.lua       -- globalstep loop; needs cadence-aware stepping
    nodes.lua           -- node registrations; add cadence color nodes here
    cadence.lua         -- NEW submodule: SeamPlane node + cadence renderer
    commands.lua        -- chat commands; add /va_cadence_* here
    mod.conf            -- add optional_depends = mesecons

~/.var/app/org.luanti.luanti/.minetest/mods/
  voxel_automata -> /var/home/kirrim/git/special-waddle/mod   (symlink)
  mesecons/            -- mesecons modpack (already installed)
    mesecons/          -- core API: mesecon.rules, mesecon.state
    mesecons_pistons/  -- reference for facedir + action_on/action_off pattern
```

Build: `cd rust && cargo build --release`
The symlink at `mod/lib/libvoxel_automata.so` picks up the new binary automatically.

---

## Mesecons Integration Reference

`mod.conf` needs `optional_depends = mesecons`. Gate all mesecons calls behind
`if minetest.get_modpath("mesecons") then`.

A Mesecons effector node registers `mesecons.effector` in its node definition:

```lua
mesecons = {
    effector = {
        action_on  = function(pos, node) ... end,
        action_off = function(pos, node) ... end,
        rules = mesecon.rules.alldirs,  -- or a custom subset
    }
}
```

Facing direction on placement (same pattern as pistons):
```lua
on_place = function(itemstack, placer, pointed_thing)
    local node = minetest.get_node(pointed_thing.under)
    -- param2 encodes the facing axis
    local param2 = core.dir_to_facedir(placer:get_look_dir(), true)
    minetest.set_node(pointed_thing.above, {name = "voxel_automata:seam_off", param2 = param2})
    return itemstack
end,
paramtype2 = "facedir",
```

Decode the axis from param2 at action time:
```lua
local dir = minetest.facedir_to_dir(node.param2)
-- dir is a {x,y,z} unit vector; the nonzero component is the seam axis
-- x!=0 -> axis=0, y!=0 -> axis=1, z!=0 -> axis=2
```

Source to read: `~/.var/app/org.luanti.luanti/.minetest/mods/mesecons/mesecons_pistons/init.lua`
(lines 44, 124 for the facedir pattern; lines 95-103 for swap_node on/off)

---

## Rust FFI to Add: `rust/src/ffi/cadence.rs`

Six new `#[no_mangle] pub extern "C"` functions. Add `pub mod cadence;` to
`rust/src/ffi/mod.rs` and re-export from `rust/src/lib.rs`.

### `va_sc_cadence_advance`

```c
// Advance cadence partition one global tick.
// Writes firing zones into caller-supplied flat arrays (max_zones capacity).
// Returns number of zones that fired this tick (0 = nothing stepped this tick).
// out_zone_data layout per zone: [min_x, min_y, min_z, max_x, max_y, max_z, cadence] (7 x i16)
uint32_t va_sc_cadence_advance(StepController* ctrl,
                                int16_t* out_zone_data, uint32_t max_zones);
```

Implementation: calls `ctrl.cadence_partition.advance()`, writes GAAABB + cadence into
the flat array, returns count. The Lua side pre-allocates e.g. 64 zones.

### `va_sc_cadence_step`

```c
// Convenience: advance one tick, then step_zones_blocking on whatever fired.
// Returns number of zones stepped (0 = nothing fired this tick).
uint32_t va_sc_cadence_step(StepController* ctrl);
```

Implementation: `advance()` → if non-empty, `step_zones_blocking(&firing)`.

### `va_sc_cadence_bisect`

```c
// Bisect the leaf containing (px,py,pz) at the given axis and coord.
// lo_cadence applies to the low side, hi_cadence to the high side.
// Also registers Buffered contracts on the seam face-pairs (via delta_overrides).
// Returns 0 on success, -1 on failure (e.g. point out of bounds).
int32_t va_sc_cadence_bisect(StepController* ctrl,
                              int16_t px, int16_t py, int16_t pz,
                              uint8_t axis, int16_t coord,
                              uint16_t lo_cadence, uint16_t hi_cadence);
```

Implementation: `cadence_partition.bisect([px,py,pz], axis, coord, lo, 0, hi, 0)`,
then iterate `seam.face_pairs(w,h,d)` and insert `NeighborKind::Buffered{drain_every}`
into `delta_overrides` for each pair.

### `va_sc_cadence_merge_poll`

```c
// Poll the merge of the two leaves containing null_point and alt_point.
// Call once per global tick (after va_sc_cadence_step) until it returns 1.
// Returns: 1 = merge complete (seam dissolved), 0 = still syncing, -1 = error.
int32_t va_sc_cadence_merge_poll(StepController* ctrl,
                                  int16_t null_x, int16_t null_y, int16_t null_z,
                                  int16_t alt_x,  int16_t alt_y,  int16_t alt_z);
```

Implementation: wraps `cadence_partition.merge(null_point, alt_point)`.
Returns 1 on `SyncStatus::Done`, 0 on `SyncStatus::Syncing`.
On Done: also deregister the Buffered contracts that `bisect` inserted.

The caller (Lua SeamPlane action_off handler) stores the two representative points in
node metadata and queues a pending merge. The animation loop calls this once per tick
until it returns 1.

### `va_sc_cadence_lookup`

```c
// Return the cadence period of the zone containing (x,y,z). Returns 0 on error.
uint16_t va_sc_cadence_lookup(StepController* ctrl, int16_t x, int16_t y, int16_t z);
```

### `va_sc_global_tick`

```c
// Return the current global_tick counter.
uint64_t va_sc_global_tick(const StepController* ctrl);
```

---

## `init.lua` cdef additions

```lua
-- Phase 9c: Cadence FFI
uint32_t va_sc_cadence_advance(StepController* ctrl, int16_t* out_zone_data, uint32_t max_zones);
uint32_t va_sc_cadence_step(StepController* ctrl);
int32_t  va_sc_cadence_bisect(StepController* ctrl,
                               int16_t px, int16_t py, int16_t pz,
                               uint8_t axis, int16_t coord,
                               uint16_t lo_cadence, uint16_t hi_cadence);
int32_t  va_sc_cadence_merge_poll(StepController* ctrl,
                                   int16_t null_x, int16_t null_y, int16_t null_z,
                                   int16_t alt_x,  int16_t alt_y,  int16_t alt_z);
uint16_t va_sc_cadence_lookup(StepController* ctrl, int16_t x, int16_t y, int16_t z);
uint64_t va_sc_global_tick(const StepController* ctrl);
```

Add `dofile(modpath .. "/cadence.lua")(M)` after `nodes.lua` and before `animation.lua`.

---

## `animation.lua` Changes

Replace the `va_sc_begin_step` / `va_sc_tick` path with `va_sc_cadence_step`:

```lua
-- Each globalstep tick:
if M.animation_state.running then
    -- Drive one global tick of cadence-aware stepping
    local zones_stepped = va.va_sc_cadence_step(M.global_step_controller)

    -- Poll any pending merge
    if M.pending_merge then
        local pm = M.pending_merge
        local result = va.va_sc_cadence_merge_poll(
            M.global_step_controller,
            pm.null_x, pm.null_y, pm.null_z,
            pm.alt_x,  pm.alt_y,  pm.alt_z)
        if result == 1 then
            M.pending_merge = nil
            minetest.log("action", "[voxel_automata] Merge complete")
            M.render_cadence_zones()
        end
    end

    -- Re-render field and cadence overlay every N ticks
    M.animation_state.render_countdown = (M.animation_state.render_countdown or 0) - 1
    if M.animation_state.render_countdown <= 0 then
        M.animation_state.render_countdown = M.animation_state.render_every
        M.render_field_grayscale()
        M.render_cadence_zones()
    end
end
```

`render_every` defaults to 16 (tunable). Remove the old `field_stepping` flag and
`va_sc_begin_step` / `va_sc_tick` paths — they are replaced entirely.

---

## Cadence Color Palette

Drop a horizontal PNG strip at `mod/textures/voxel_automata_cadence_palette.png`.
Width = number of colors (suggest 32), height = 1. Color index 0 = cadence 1 (fastest,
e.g. bright yellow), index 31 = cadence 32 (slowest, e.g. deep blue).

Register N cadence nodes in `nodes.lua` (inside an existing loop or new block):

```lua
local CADENCE_COLORS = 32
for i = 1, CADENCE_COLORS do
    minetest.register_node(string.format("voxel_automata:cadence_%02d", i), {
        description = string.format("Cadence Zone (period %d)", i),
        tiles = { {
            name = "voxel_automata_cadence_palette.png",
            -- Select column i-1 from the 1-pixel-tall strip
            animation = { type = "vertical_frames", aspect_w = 1, aspect_h = 1,
                          length = CADENCE_COLORS },
        } },
        -- simpler: just use colored overrides per-node, or a single tinted tile
        walkable = false,
        pointable = false,
        sunlight_propagates = true,
        buildable_to = true,
        groups = { not_in_creative_inventory = 1 },
    })
end
```

Simpler alternative if the palette approach is fiddly: use `color = {r,g,b}` override
on a plain white tile, computing the color from a hardcoded Lua table of 32 RGB values.
The player provides the palette as a 32-entry Lua table in `cadence.lua`.

---

## Cadence Zone Renderer (`mod/cadence.lua` → `M.render_cadence_zones`)

The cadence overlay renders at:

```
world_x = M.viewport_anchor.x + math.ceil(field_w / 16) * 16
world_y = M.viewport_anchor.y
world_z = M.viewport_anchor.z
```

(One mapblock-aligned step in +x from the grayscale mass field. The grayscale field sits
at the viewport anchor itself; cadence zones sit one mapblock to its right.)

The renderer fills the bounding box of each leaf GAAABB with the node for its cadence:

```lua
function M.render_cadence_zones()
    if not M.global_step_controller then return end

    local field_w = 16  -- or M.field_size_x when that is parameterized
    local ox = M.viewport_anchor.x + math.ceil(field_w / 16) * 16
    local oy = M.viewport_anchor.y
    local oz = M.viewport_anchor.z

    -- Walk the field; for each voxel query its cadence, pick the node.
    -- Using VoxelManip for bulk writes.
    -- va_sc_cadence_lookup called per-voxel is fine for 16^3 = 4096 calls.
    local vm = VoxelManip()
    local world_min = {x=ox, y=oy, z=oz}
    local world_max = {x=ox+field_w-1, y=oy+field_w-1, z=oz+field_w-1}
    local emin, emax = vm:read_from_map(world_min, world_max)
    local data = vm:get_data()
    local area = VoxelArea:new({MinEdge=emin, MaxEdge=emax})

    local cadence_ids = {}
    for i = 1, 32 do
        cadence_ids[i] = minetest.get_content_id(
            string.format("voxel_automata:cadence_%02d", i))
    end
    local air_id = minetest.get_content_id("air")

    for z = 0, field_w-1 do
        for y = 0, field_w-1 do
            for x = 0, field_w-1 do
                local c = va.va_sc_cadence_lookup(M.global_step_controller, x, y, z)
                local node_id = cadence_ids[math.min(c, 32)] or air_id
                local vi = area:indexp({x=ox+x, y=oy+y, z=oz+z})
                data[vi] = node_id
            end
        end
    end

    vm:set_data(data)
    vm:write_to_map()
    vm:update_map()
end
```

---

## SeamPlane Node (`mod/cadence.lua`)

Two node variants: `voxel_automata:seam_off` and `voxel_automata:seam_on`
(off = deenergized, on = energized). Swap between them on action_on/action_off,
matching the piston pattern.

**Cadence assignment formula** — used when bisecting to assign cadence to each leaf:

For a leaf covering a GAAABB, compute the Euclidean distance from the field origin
`[0,0,0]` to each of its 8 corners, take the maximum. Use `math.floor(max_dist / scale) + 1`
as the cadence, clamped to `[1, 32]`. The scale factor defaults to
`field_diagonal / 32` so the full range of 32 cadences spans the field.

In practice the SeamPlane node assigns cadences at bisect time. The lo side gets the
cadence computed from its half of the GAAABB; the hi side gets its own. Because the
SeamPlane is the bisect point, the two representative field-coords are
`(seam_coord - 1, ...)` and `(seam_coord, ...)` on either side of the cut.

**action_on (bisect):**
```lua
action_on = function(pos, node)
    if not M.global_step_controller then return end
    local dir = minetest.facedir_to_dir(node.param2)
    local axis = dir.x ~= 0 and 0 or (dir.y ~= 0 and 1 or 2)
    -- convert world pos to field coord
    local fx = pos.x - M.viewport_anchor.x + math.ceil(field_w/16)*16
    -- (note: seam_on sits in the world at the cadence overlay offset, not the field offset)
    -- simpler: store field coord in metadata at on_place time
    local meta = minetest.get_meta(pos)
    local fcx = meta:get_int("field_x")
    local fcy = meta:get_int("field_y")
    local fcz = meta:get_int("field_z")
    local seam_coord = axis==0 and fcx or (axis==1 and fcy or fcz)
    local lo_cadence = compute_cadence_for_halfspace(axis, seam_coord, "lo")
    local hi_cadence = compute_cadence_for_halfspace(axis, seam_coord, "hi")
    va.va_sc_cadence_bisect(M.global_step_controller,
        fcx, fcy, fcz, axis, seam_coord, lo_cadence, hi_cadence)
    minetest.swap_node(pos, {name="voxel_automata:seam_on", param2=node.param2})
    M.render_cadence_zones()
end
```

**action_off (begin merge):**
```lua
action_off = function(pos, node)
    if not M.global_step_controller then return end
    local meta = minetest.get_meta(pos)
    local fcx = meta:get_int("field_x")
    local fcy = meta:get_int("field_y")
    local fcz = meta:get_int("field_z")
    local dir = minetest.facedir_to_dir(node.param2)
    local axis = dir.x ~= 0 and 0 or (dir.y ~= 0 and 1 or 2)
    -- null_point = one voxel on low side, alt_point = one voxel on high side
    local null_x = fcx - (axis==0 and 1 or 0)
    local null_y = fcy - (axis==1 and 1 or 0)
    local null_z = fcz - (axis==2 and 1 or 0)
    M.pending_merge = {
        null_x=null_x, null_y=null_y, null_z=null_z,
        alt_x=fcx, alt_y=fcy, alt_z=fcz
    }
    minetest.swap_node(pos, {name="voxel_automata:seam_off", param2=node.param2})
    minetest.log("action", "[voxel_automata] Merge queued, polling until phase converges")
end
```

The animation loop calls `va_sc_cadence_merge_poll` each tick until it returns 1,
then calls `render_cadence_zones()` to reflect the dissolved seam.

**on_place** (store field coord in metadata):
```lua
on_place = function(itemstack, placer, pointed_thing)
    local above = pointed_thing.above
    local param2 = core.dir_to_facedir(placer:get_look_dir(), true)
    minetest.set_node(above, {name="voxel_automata:seam_off", param2=param2})
    local meta = minetest.get_meta(above)
    -- field coords: world pos minus field anchor offset
    -- (cadence overlay is offset by one mapblock in x, so subtract that back)
    local ox = M.viewport_anchor.x + math.ceil(16/16)*16
    meta:set_int("field_x", above.x - ox)
    meta:set_int("field_y", above.y - M.viewport_anchor.y)
    meta:set_int("field_z", above.z - M.viewport_anchor.z)
    return itemstack
end,
```

## Infinity (contract node)s

5 nodes, ask the user to paint some textures for them, which when energized with Mese (action_on) will make the Infinity Contract (delta.rs/pub enum ContractKind) with preset values which are 5%, 25%, 50%, 75%, and 95% of u32 max value. The contract can be destroyed or set to zero conductivity when action_off is called.

all of those nodes should be able to act from outside the field because they would be destroyed by the renderer if they are placed inside the field.

---

## Chat Commands to Add (`mod/commands.lua`)

| Command | Description |
|---|---|
| `/va_cadence_show` | Force-render the cadence zone overlay now |
| `/va_cadence_info` | Print per-zone cadence, accumulator, GAAABB bounds |
| `/va_cadence_animate [on\|off]` | Enable/disable cadence-aware stepping in the animation loop |

---

## Visualization Layout Summary

```
viewport_anchor (x, y, z)
  ├── +y+50 (old):  grayscale mass field  [was render_field_grayscale]
  │                 (may be moved to y+0 now that field is the primary view)
  └── +x + ceil(field_w/16)*16:  cadence zone overlay  [render_cadence_zones]
      Each voxel colored by its leaf's cadence period (1=fast/yellow, 32=slow/blue)
```

The two volumes never overlap. The player stands between them to compare mass
distribution with the cadence gradient that drives it.

---

## Open Questions / Not In Scope for 9c

- **Hotspot node** (Phase 10): the SeamPlane is a manual stand-in. A proper
  `RefinementAnchor` trait in Rust (emitting `Vec<(Gaaabb, cadence)>`) and an
  auto-bisect algorithm are Phase 9c's test subject, not yet implemented.
- **Mesecons timer circuit**: the player builds a gate-based oscillator to move the
  SeamPlane's Mese signal, driving the seam position over time. This is pure in-world
  construction, no code changes needed.
- **Non-blocking cadence step**: `va_sc_cadence_step` is currently blocking. A future
  phase may split it into begin/tick like the old incremental path.
- **`va_destroy_field` FFI**: `va_destroy_field` is already exported but
  `va_sc_cadence_bisect`'s Buffered contract cleanup on coarsen/merge needs care to
  not leave dangling entries in `delta_overrides`.

## In-game bug pass (post 9c first playtest) — pending interface rework

Found by playtesting the mostly-working 9c build. Fixes below are paused because the
cadence/default-cadence interface is about to be redesigned (user has a bigger plan) —
don't apply until that lands.

- **Underflow/mass-creation bug (confirmed root cause)**: `apply_one_sided` (used by
  Infinity/Void contracts, `kernel.rs`) can compute a `flow` larger than the target
  cell's actual value when `conductivity * dt` isn't safely below `divisor` — `dt` here
  is the *cadence* used as the zone's time step (see `step_zones_blocking`, and the
  `process_contract_list` TODO that contracts currently use "the last firing zone's dt").
  When that bound is violated the `u32` subtraction in `apply_one_sided`/`apply_pair`
  wraps to near-`u32::MAX`, fabricating mass. This reproduces "messing with infinity
  nodes and cadence causes underflow." Fix direction (agreed): enforce the bound at
  cadence-*assignment* time (reject unsafe cadences in `va_sc_cadence_bisect`), not with
  a per-step runtime clamp in the hot path.
- **Max/default cadence not well-defined (confirmed, and worse than assumed)**: The
  theoretical per-pair stability bound (`divisor/conductivity`, e.g. 111 for
  diffusion_rate=4) is NOT the real safe limit — a cell can own up to 3 overridden pairs
  sharing one tile's `remainder_acc`, so real stability breaks much earlier. Built a
  generalized empirical sweep for this in
  `rust/src/tests/physics.rs::test_find_max_stable_cadence_per_diffusion_rate`
  (helpers: `build_ctrl_rate`, `cadence_is_stable`, `find_max_stable_cadence` — reusable
  for any diffusion_rate, and later, per-material conductivity for water/ice/steam).
  Measured max stable cadence by diffusion_rate (current fixed conductivity=65535):
  `[1, 2, 5, 11, 22, 46, 93, 196, 432]` for rate 0–8 (rate 8 not exhaustively searched
  above 432). This roughly matches the existing (currently failing)
  `test_time_constant_preserved_under_cadence_change`, which found instability at
  cadence 25 for diffusion_rate=4 against a naive-formula expectation of 111.
  In-game field is created at diffusion_rate=2 (`mod/commands.lua` `/va_create_field`),
  whose measured safe bound is only **5** — but `cadence.lua`'s
  `compute_cadence_for_halfspace` currently produces values up to ~9 (and the palette
  goes up to 32), so the SeamPlane can already assign unsafe cadences in normal play.
  Proposed fix (not yet applied, pending the interface rework): a
  `kernel::max_safe_cadence(diffusion_rate)` lookup sourced from the measured table,
  enforced as a hard refusal in `va_sc_cadence_bisect` (reject, don't clamp), plus an FFI
  getter so `cadence.lua`'s formula can size its output to the real bound instead of a
  hardcoded 32.
- **Merge queue bug (confirmed by reading code, not yet fixed)**: `M.pending_merge`
  (`mod/cadence.lua`, `mod/animation.lua`) is a single slot, not a queue/set. If a second
  SeamPlane's `action_off` fires while a merge is already pending, it silently
  overwrites `M.pending_merge` — the first merge's Buffered contracts on its seam are
  never polled to completion or cleaned up (leaked `delta_overrides` entries, seam stuck
  mid-sync forever). Needs `M.pending_merges` keyed by seam identity (e.g. node pos),
  animation loop polls all of them, and `action_on`/bisect should refuse to re-bisect a
  plane that's already mid-merge rather than corrupt the KD-tree's assumed state —
  surfaced as a clear refusal so the Lua caller knows to reconsider, not a silent no-op.
- **"min cadence might be too slow" (not yet root-caused)**: likely about
  `compute_cadence_for_halfspace`'s slow-zone output (up to 32) feeling too sluggish in
  practice for a 16-wide field, rather than a logic bug — needs the user's call on
  desired feel once the cadence-assignment interface is redesigned.
- **Hotspot-tied-to-entity feature request**: deferred until after this bug pass per
  user (see conversation) — needs a 3rd render-cube (edges only) tracking a tagged
  Luanti entity; open question on the Luanti side is which entity/mod to move it with
  (mentioned minecart/rollercoaster mod as a possible mount).
