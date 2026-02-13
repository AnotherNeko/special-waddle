-- Voxel Automata: 3D Cellular Automata for Luanti
-- Phase 4: Visualize

local modname = minetest.get_current_modname()
local modpath = minetest.get_modpath(modname)

-- Get insecure environment to access FFI
local ie = minetest.request_insecure_environment()
if not ie then
    error("voxel_automata requires access to insecure environment. Add it to secure.trusted_mods in minetest.conf")
end

-- Load FFI
local ffi = ie.require("ffi")

-- Declare C function signatures
ffi.cdef [[
    // Phase 1
    int32_t va_add(int32_t a, int32_t b);

    // Phase 2: Opaque handle lifecycle
    typedef struct State State;
    State* va_create(void);
    void va_destroy(State* ptr);
    uint64_t va_get_generation(const State* ptr);

    // Phase 3: Small grid + step
    int32_t va_create_grid(State* ptr, int16_t width, int16_t height, int16_t depth);
    void va_set_cell(State* ptr, int16_t x, int16_t y, int16_t z, uint8_t alive);
    uint8_t va_get_cell(const State* ptr, int16_t x, int16_t y, int16_t z);
    void va_step(State* ptr);

    // Phase 4: Visualize
    uint64_t va_extract_region(const State* ptr, uint8_t* out_buf,
                                int16_t min_x, int16_t min_y, int16_t min_z,
                                int16_t max_x, int16_t max_y, int16_t max_z);

    // Phase 5: Bidirectional sync
    uint64_t va_import_region(State* ptr, const uint8_t* in_buf,
                               int16_t min_x, int16_t min_y, int16_t min_z,
                               int16_t max_x, int16_t max_y, int16_t max_z);

    // Phase 6: Integer Field + Delta Diffusion
    typedef struct Field Field;
    Field* va_create_field(int16_t width, int16_t height, int16_t depth, uint8_t diffusion_rate);
    void va_destroy_field(Field* ptr);
    void va_field_set(Field* ptr, int16_t x, int16_t y, int16_t z, uint32_t value);
    uint32_t va_field_get(const Field* ptr, int16_t x, int16_t y, int16_t z);
    void va_field_step(Field* ptr);
    uint64_t va_field_get_generation(const Field* ptr);

    // Phase 8a: Non-blocking incremental stepping
    typedef struct StepController StepController;
    StepController* va_create_step_controller(int16_t w, int16_t h, int16_t d, uint8_t diffusion_rate, uint8_t num_threads);
    void va_destroy_step_controller(StepController* ctrl);
    void va_sc_field_set(StepController* ctrl, int16_t x, int16_t y, int16_t z, uint32_t value);
    uint32_t va_sc_field_get(const StepController* ctrl, int16_t x, int16_t y, int16_t z);
    uint64_t va_sc_field_get_generation(const StepController* ctrl);
    int32_t va_sc_begin_step(StepController* ctrl);
    int32_t va_sc_tick(StepController* ctrl, uint64_t budget_us);
    int32_t va_sc_is_stepping(const StepController* ctrl);
    void va_sc_step_blocking(StepController* ctrl);
]]

-- Load the Rust library
local lib_path = modpath .. "/lib/libvoxel_automata.so"
local va = ffi.load(lib_path)

-- Global state holder (initialized later)
local global_state = nil

-- ============================================================================
-- Phase 5: Coordinate Mapping & Animation State
-- ============================================================================

-- Viewport anchor: world position corresponding to automaton (0,0,0)
local viewport_anchor = { x = 0, y = 0, z = 0 }
local grid_size = 16

-- Coordinate conversion helpers
local function world_to_automaton(world_pos)
    return {
        x = world_pos.x - viewport_anchor.x,
        y = world_pos.y - viewport_anchor.y,
        z = world_pos.z - viewport_anchor.z
    }
end

local function automaton_to_world(auto_pos)
    return {
        x = auto_pos.x + viewport_anchor.x,
        y = auto_pos.y + viewport_anchor.y,
        z = auto_pos.z + viewport_anchor.z
    }
end

local function is_in_automaton_bounds(auto_pos)
    return auto_pos.x >= 0 and auto_pos.x < grid_size
        and auto_pos.y >= 0 and auto_pos.y < grid_size
        and auto_pos.z >= 0 and auto_pos.z < grid_size
end

-- Animation state
local animation_state = {
    running = false,
    interval = 1.0,         -- seconds between steps
    timer = 0.0,
    field_stepping = false, -- Phase 8a: track if StepController is mid-step
}

-- Phase 4/5: Register node type for visualization with interaction callbacks
minetest.register_node("voxel_automata:cell", {
    description = "Cellular Automata Cell",
    tiles = { "voxel_automata_cell.png" },
    walkable = false,
    sunlight_propagates = true,
    groups = { dig_immediate = 3, not_in_creative_inventory = 1 },

    -- Phase 5: Sync cell removal back to automaton
    on_dig = function(pos, node, digger)
        if global_state then
            local auto_pos = world_to_automaton(pos)
            if is_in_automaton_bounds(auto_pos) then
                va.va_set_cell(global_state, auto_pos.x, auto_pos.y, auto_pos.z, 0)
                minetest.log("action", "[voxel_automata] Cell dug at automaton " .. minetest.pos_to_string(auto_pos))
            end
        end
        minetest.node_dig(pos, node, digger)
    end,
})

-- Phase 5: Sync cell placement back to automaton
minetest.register_on_placenode(function(pos, newnode, placer, oldnode, itemstack, pointed_thing)
    if newnode.name == "voxel_automata:cell" and global_state then
        local auto_pos = world_to_automaton(pos)
        if is_in_automaton_bounds(auto_pos) then
            va.va_set_cell(global_state, auto_pos.x, auto_pos.y, auto_pos.z, 1)
            minetest.log("action", "[voxel_automata] Cell placed at automaton " .. minetest.pos_to_string(auto_pos))
        end
    end
end)

-- ============================================================================
-- Phase 8b: Register 256 grayscale nodes for u32 field visualization
-- ============================================================================
-- Mapping: u32 value (0 to 4,294,967,295) → grayscale (0 to 255)
-- Formula: grayscale = math.floor(value / 16777216)  -- divide by 2^24

for i = 0, 255 do
    local brightness = i / 255.0
    local node_name = string.format("voxel_automata:mass_%03d", i)

    minetest.register_node(node_name, {
        description = string.format("Mass Field (level %d/255)", i),
        tiles = { {
            name = "voxel_automata_grayscale.png",
            color = { r = brightness * 255, g = brightness * 255, b = brightness * 255 },
        } },
        paramtype = "light",
        light_source = 0,
        sunlight_propagates = true,
        walkable = false,
        pointable = false,
        buildable_to = true,
        groups = { not_in_creative_inventory = 1 },
    })
end

minetest.log("action", "[voxel_automata] Phase 8b: Registered 256 grayscale mass nodes")

-- Helper function to render a region using individual node placement
-- TODO: Replace with VoxelManip variant once timing issues are resolved
local function render_region(min_x, min_y, min_z, max_x, max_y, max_z)
    if not global_state then
        minetest.log("warning", "[voxel_automata] Cannot render: global_state is nil")
        return
    end

    -- Calculate dimensions
    local width = max_x - min_x
    local height = max_y - min_y
    local depth = max_z - min_z

    -- Create buffer for extraction
    local buffer_size = width * height * depth
    local buffer = ffi.new("uint8_t[?]", buffer_size)

    -- Extract region from automaton
    local bytes_written = va.va_extract_region(
        global_state,
        buffer,
        min_x, min_y, min_z,
        max_x, max_y, max_z
    )

    if bytes_written == 0 then
        minetest.log("warning", "[voxel_automata] Extract region returned 0 bytes")
        return
    end

    -- Place nodes directly (simpler, slower approach for now)
    local offset = 0
    local placed_count = 0
    for z = min_z, max_z - 1 do
        for y = min_y, max_y - 1 do
            for x = min_x, max_x - 1 do
                local cell_state = buffer[offset]
                if cell_state == 1 then
                    minetest.set_node({ x = x, y = y, z = z }, { name = "voxel_automata:cell" })
                    placed_count = placed_count + 1
                end
                offset = offset + 1
            end
        end
    end

    minetest.log("action", "[voxel_automata] Placed " .. placed_count .. " nodes")
end

-- Helper function to render automaton grid at a specific world position
local function render_region_at_world(min_x, min_y, min_z, max_x, max_y, max_z, world_x, world_y, world_z)
    if not global_state then
        minetest.log("warning", "[voxel_automata] Cannot render: global_state is nil")
        return
    end

    -- Calculate dimensions
    local width = max_x - min_x
    local height = max_y - min_y
    local depth = max_z - min_z

    -- Create buffer for extraction
    local buffer_size = width * height * depth
    local buffer = ffi.new("uint8_t[?]", buffer_size)

    -- Extract region from automaton
    local bytes_written = va.va_extract_region(
        global_state,
        buffer,
        min_x, min_y, min_z,
        max_x, max_y, max_z
    )

    if bytes_written == 0 then
        minetest.log("warning", "[voxel_automata] Extract region returned 0 bytes")
        return
    end

    -- Place nodes at world coordinates (always set, either air or cell)
    local offset = 0
    local placed_count = 0
    for z = min_z, max_z - 1 do
        for y = min_y, max_y - 1 do
            for x = min_x, max_x - 1 do
                local cell_state = buffer[offset]
                local node_name = cell_state == 1 and "voxel_automata:cell" or "air"
                minetest.set_node(
                    { x = world_x + (x - min_x), y = world_y + (y - min_y), z = world_z + (z - min_z) },
                    { name = node_name }
                )
                if cell_state == 1 then
                    placed_count = placed_count + 1
                end
                offset = offset + 1
            end
        end
    end

    minetest.log("action",
        "[voxel_automata] Placed " ..
        placed_count .. " nodes at world (" .. world_x .. "," .. world_y .. "," .. world_z .. ")")
end

-- VoxelManip variant for bulk operations (optimized alternative to render_region_at_world)
local function render_region_at_world_voxelmanip(min_x, min_y, min_z, max_x, max_y, max_z, world_x, world_y, world_z)
    if not global_state then
        minetest.log("warning", "[voxel_automata] Cannot render: global_state is nil")
        return
    end

    -- Calculate dimensions
    local width = max_x - min_x
    local height = max_y - min_y
    local depth = max_z - min_z

    -- Create buffer for extraction
    local buffer_size = width * height * depth
    local buffer = ffi.new("uint8_t[?]", buffer_size)

    -- Extract region from automaton
    local bytes_written = va.va_extract_region(
        global_state,
        buffer,
        min_x, min_y, min_z,
        max_x, max_y, max_z
    )

    if bytes_written == 0 then
        minetest.log("warning", "[voxel_automata] Extract region returned 0 bytes")
        return
    end

    -- Calculate world coordinates
    local world_min = { x = world_x, y = world_y, z = world_z }
    local world_max = { x = world_x + width - 1, y = world_y + height - 1, z = world_z + depth - 1 }

    -- Create VoxelManip for bulk writing
    local vm = VoxelManip()
    local emerged_min, emerged_max = vm:read_from_map(world_min, world_max)

    local data = vm:get_data()
    local area = VoxelArea:new({ MinEdge = emerged_min, MaxEdge = emerged_max })

    -- Get content IDs
    local node_id = minetest.get_content_id("voxel_automata:cell")
    local air_id = minetest.get_content_id("air")

    -- Fill data array from buffer
    local offset = 0
    local placed_count = 0
    for z = min_z, max_z - 1 do
        for y = min_y, max_y - 1 do
            for x = min_x, max_x - 1 do
                local cell_state = buffer[offset]
                local world_pos = {
                    x = world_x + (x - min_x),
                    y = world_y + (y - min_y),
                    z = world_z + (z - min_z)
                }
                local vi = area:indexp(world_pos)
                data[vi] = cell_state == 1 and node_id or air_id
                if cell_state == 1 then
                    placed_count = placed_count + 1
                end
                offset = offset + 1
            end
        end
    end

    -- Write back to map
    vm:set_data(data)
    vm:write_to_map()
    vm:update_map()

    minetest.log("action",
        "[voxel_automata] VoxelManip: Placed " ..
        placed_count .. " nodes at world (" .. world_x .. "," .. world_y .. "," .. world_z .. ")")
end

-- ============================================================================
-- Initialization and Testing
-- ============================================================================

-- Phase 1 test: Call va_add and print result
local a = 2
local b = 3
local result = va.va_add(a, b)
minetest.log("action", "[voxel_automata] Phase 1: " .. a .. " + " .. b .. " = " .. result)

-- Phase 2 test: Create handle, query generation, and prepare for cleanup
local state = va.va_create()
if state == nil then
    error("[voxel_automata] Failed to create state")
end

local generation = va.va_get_generation(state)
minetest.log("action", "[voxel_automata] Phase 2: Created state, generation = " .. tonumber(generation))

-- Store in global for use by rendering functions
global_state = state

-- Phase 3 test: Create a small grid and test basic operations
local grid_result = va.va_create_grid(state, 16, 16, 16)
if grid_result ~= 0 then
    error("[voxel_automata] Failed to create grid")
end
minetest.log("action", "[voxel_automata] Phase 3: Created 16x16x16 grid")

-- Set some cells alive in a cross pattern
va.va_set_cell(state, 8, 8, 8, 1) -- Center
va.va_set_cell(state, 7, 8, 8, 1) -- Left
va.va_set_cell(state, 9, 8, 8, 1) -- Right
va.va_set_cell(state, 8, 7, 8, 1) -- Front
va.va_set_cell(state, 8, 9, 8, 1) -- Back
minetest.log("action", "[voxel_automata] Phase 3: Set 5 cells alive (cross pattern)")

-- Count alive cells
local alive_count = 0
for z = 0, 15 do
    for y = 0, 15 do
        for x = 0, 15 do
            if va.va_get_cell(state, x, y, z) == 1 then
                alive_count = alive_count + 1
            end
        end
    end
end
minetest.log("action", "[voxel_automata] Phase 3: Initial alive count = " .. alive_count)

-- Store for /ca_test command
local alive_count_after = alive_count

-- ============================================================================
-- Phase 6 test: Integer Field + Delta Diffusion
-- ============================================================================

local global_field = va.va_create_field(16, 16, 16, 2)
if global_field == nil then
    error("[voxel_automata] Failed to create field")
end
minetest.log("action", "[voxel_automata] Phase 6: Created 16x16x16 field (diffusion_rate=2)")

-- Set a point source
va.va_field_set(global_field, 8, 8, 8, 1000000)
local initial_value = va.va_field_get(global_field, 8, 8, 8)
minetest.log("action", "[voxel_automata] Phase 6: Set point source to " .. initial_value)

-- Step once and check diffusion
va.va_field_step(global_field)
local center_after = va.va_field_get(global_field, 8, 8, 8)
local neighbor_x = va.va_field_get(global_field, 7, 8, 8)
local neighbor_y = va.va_field_get(global_field, 8, 7, 8)
local neighbor_z = va.va_field_get(global_field, 8, 8, 7)

minetest.log("action", "[voxel_automata] Phase 6: After 1 step:")
minetest.log("action", "  Center (8,8,8): " .. center_after)
minetest.log("action", "  Neighbor X (7,8,8): " .. neighbor_x)
minetest.log("action", "  Neighbor Y (8,7,8): " .. neighbor_y)
minetest.log("action", "  Neighbor Z (8,8,7): " .. neighbor_z)

local gen = va.va_field_get_generation(global_field)
minetest.log("action", "[voxel_automata] Phase 6: Field generation = " .. tonumber(gen))

-- ============================================================================
-- Phase 8a: Create StepController for non-blocking field stepping
-- ============================================================================

local global_step_controller = va.va_create_step_controller(16, 16, 16, 2, 1)
if global_step_controller == nil then
    error("[voxel_automata] Failed to create step controller")
end
minetest.log("action", "[voxel_automata] Phase 8a: Created StepController (16x16x16, diffusion_rate=2, 1 thread)")

-- Set a point source for testing at corner (0,0,0) with max brightness
-- u32 max value = 4,294,967,295 (maps to grayscale 255, white)
va.va_sc_field_set(global_step_controller, 0, 0, 0, 3999995905)
local sc_initial_value = va.va_sc_field_get(global_step_controller, 0, 0, 0)
minetest.log("action", "[voxel_automata] Phase 8a: Set point source to " .. sc_initial_value)

-- ============================================================================
-- Phase 8b: Render u32 field as grayscale blocks
-- ============================================================================

local function render_field_grayscale()
    if not global_step_controller then
        return
    end

    -- Define field viewport (offset from CA viewport for clarity)
    local field_anchor = {
        x = viewport_anchor.x,
        y = viewport_anchor.y + 50,
        z = viewport_anchor.z
    }

    local field_size = 16 -- Match StepController dimensions

    -- Calculate world bounds
    local world_min = field_anchor
    local world_max = {
        x = field_anchor.x + field_size - 1,
        y = field_anchor.y + field_size - 1,
        z = field_anchor.z + field_size - 1
    }

    -- Create VoxelManip for bulk writing
    local vm = VoxelManip()
    local emerged_min, emerged_max = vm:read_from_map(world_min, world_max)
    local data = vm:get_data()
    local area = VoxelArea:new({ MinEdge = emerged_min, MaxEdge = emerged_max })

    -- Pre-cache grayscale node content IDs
    local grayscale_ids = {}
    for i = 0, 255 do
        local node_name = string.format("voxel_automata:mass_%03d", i)
        grayscale_ids[i] = minetest.get_content_id(node_name)
    end
    local air_id = minetest.get_content_id("air")

    -- Read field data and map to grayscale nodes
    local nonzero_count = 0
    for z = 0, field_size - 1 do
        for y = 0, field_size - 1 do
            for x = 0, field_size - 1 do
                local value = va.va_sc_field_get(global_step_controller, x, y, z)

                -- Map u32 to 0-255 grayscale
                local grayscale = math.floor(value / 16777216) -- Divide by 2^24
                if grayscale > 255 then grayscale = 255 end

                local world_pos = {
                    x = field_anchor.x + x,
                    y = field_anchor.y + y,
                    z = field_anchor.z + z
                }
                local vi = area:indexp(world_pos)

                -- Place grayscale node if nonzero, air if zero
                data[vi] = grayscale > 0 and grayscale_ids[grayscale] or air_id

                if value > 0 then
                    nonzero_count = nonzero_count + 1
                end
            end
        end
    end

    -- Write back to map
    vm:set_data(data)
    vm:write_to_map()
    vm:update_map()

    minetest.log("action", string.format("[voxel_automata] Field rendered: %d/%d cells nonzero",
        nonzero_count, field_size * field_size * field_size))
end

minetest.log("action", "[voxel_automata] Loaded successfully!")

-- ============================================================================
-- Phase 5: Animation System
-- ============================================================================

-- Globalstep callback for automatic stepping and non-blocking field rendering
minetest.register_globalstep(function(dtime)
    if not global_state or not global_step_controller then
        return
    end

    -- Phase 8a: Handle ongoing incremental step (non-blocking work)
    if animation_state.field_stepping then
        local done = va.va_sc_tick(global_step_controller, 4000) -- 4ms budget per tick
        if done == 1 then
            animation_state.field_stepping = false
            local gen = va.va_sc_field_get_generation(global_step_controller)
            minetest.log("action", "[voxel_automata] Incremental step completed: generation " .. tonumber(gen))

            -- Phase 8b: Render field after step completes
            render_field_grayscale()
        end
        return -- Continue processing this step, don't start new work
    end

    -- Animation timer logic (only if not mid-step)
    if not animation_state.running then
        return
    end

    animation_state.timer = animation_state.timer + dtime

    if animation_state.timer >= animation_state.interval then
        animation_state.timer = 0

        -- Step the cellular automaton (still blocking, Phase 3 behavior)
        va.va_step(global_state)

        -- Phase 8a: Begin new incremental field step (non-blocking)
        local result = va.va_sc_begin_step(global_step_controller)
        if result == 0 then -- 0 = success
            animation_state.field_stepping = true
            -- Do first tick of work immediately (avoid one-frame delay)
            local done = va.va_sc_tick(global_step_controller, 4000)
            if done == 1 then
                animation_state.field_stepping = false
                local gen = va.va_sc_field_get_generation(global_step_controller)
                minetest.log("action",
                    "[voxel_automata] Incremental step completed immediately: generation " .. tonumber(gen))

                -- Phase 8b: Render field after step completes
                render_field_grayscale()
            end
        end

        -- Render the cellular automaton using VoxelManip at viewport anchor
        render_region_at_world_voxelmanip(
            0, 0, 0,
            grid_size, grid_size, grid_size,
            viewport_anchor.x, viewport_anchor.y, viewport_anchor.z
        )

        local gen = va.va_get_generation(global_state)
        minetest.log("action", "[voxel_automata] Animation step: CA generation " .. tonumber(gen))
    end
end)

-- ============================================================================
-- Chat Commands
-- ============================================================================

-- /va info: Show automaton statistics
minetest.register_chatcommand("va_info", {
    description = "Show automaton statistics (generation, alive cells, grid size, viewport anchor)",
    func = function(name, param)
        if not global_state then
            return false, "No automaton state available"
        end

        local generation = va.va_get_generation(global_state)

        -- Count alive cells
        local alive_count = 0
        for z = 0, grid_size - 1 do
            for y = 0, grid_size - 1 do
                for x = 0, grid_size - 1 do
                    if va.va_get_cell(global_state, x, y, z) == 1 then
                        alive_count = alive_count + 1
                    end
                end
            end
        end

        minetest.chat_send_player(name, "[voxel_automata] Generation: " .. tonumber(generation))
        minetest.chat_send_player(name,
            "[voxel_automata] Alive cells: " .. alive_count .. " / " .. (grid_size * grid_size * grid_size))
        minetest.chat_send_player(name,
            "[voxel_automata] Grid size: " .. grid_size .. "x" .. grid_size .. "x" .. grid_size)
        minetest.chat_send_player(name, "[voxel_automata] Viewport anchor: " .. minetest.pos_to_string(viewport_anchor))
        minetest.chat_send_player(name,
            "[voxel_automata] Animation: " .. (animation_state.running and "running" or "stopped"))

        return true, "Info displayed"
    end
})

-- /va step: Step the automaton forward by N generations
minetest.register_chatcommand("va_step", {
    description = "Step the automaton forward by N generations. Usage: /va_step [count]",
    func = function(name, param)
        if not global_state then
            return false, "No automaton state available"
        end

        local count = tonumber(param) or 1
        if count < 1 or count > 100 then
            return false, "Count must be between 1 and 100"
        end

        for i = 1, count do
            va.va_step(global_state)
        end

        local generation = va.va_get_generation(global_state)
        return true, "Stepped " .. count .. " generation(s). Now at generation " .. tonumber(generation)
    end
})

-- /va show: Render automaton at world position (VoxelManip only)
minetest.register_chatcommand("va_show", {
    description =
    "Render automaton grid at world position using VoxelManip. Usage: /va_show [world_x] [world_y] [world_z]",
    func = function(name, param)
        if not global_state then
            return false, "No automaton state available"
        end

        local world_x, world_y, world_z = param:match("([^ ]+) ([^ ]+) ([^ ]+)")

        if not world_x or not world_y or not world_z then
            -- If no args, use player position rounded to nearest 16-block boundary
            local player = minetest.get_player_by_name(name)
            if not player then
                return false, "Player not found"
            end

            local pos = player:get_pos()
            world_x = math.floor(pos.x / 16) * 16
            world_y = math.floor(pos.y / 16) * 16
            world_z = math.floor(pos.z / 16) * 16
        else
            world_x = tonumber(world_x)
            world_y = tonumber(world_y)
            world_z = tonumber(world_z)
        end

        -- Update viewport anchor
        viewport_anchor.x = world_x
        viewport_anchor.y = world_y
        viewport_anchor.z = world_z

        -- Render using VoxelManip
        local start_time = minetest.get_us_time()
        render_region_at_world_voxelmanip(0, 0, 0, grid_size, grid_size, grid_size, world_x, world_y, world_z)
        local elapsed = (minetest.get_us_time() - start_time) / 1000

        minetest.chat_send_player(name,
            string.format("[voxel_automata] Rendered in %.2f ms at %s", elapsed, minetest.pos_to_string(viewport_anchor)))
        return true, "Automaton rendered"
    end
})

-- /va animate: Start automatic stepping and rendering
minetest.register_chatcommand("va_animate", {
    description = "Start automatic stepping and rendering. Usage: /va_animate [interval_ms]",
    func = function(name, param)
        if not global_state then
            return false, "No automaton state available"
        end

        local interval_ms = tonumber(param) or 1000
        if interval_ms < 100 or interval_ms > 10000 then
            return false, "Interval must be between 100 and 10000 milliseconds"
        end

        animation_state.running = true
        animation_state.interval = interval_ms / 1000.0
        animation_state.timer = 0

        minetest.chat_send_player(name,
            string.format("[voxel_automata] Animation started (interval: %d ms)", interval_ms))
        return true, "Animation started"
    end
})

-- /va stop: Stop animation
minetest.register_chatcommand("va_stop", {
    description = "Stop automatic animation",
    func = function(name, param)
        if not animation_state.running then
            return false, "Animation is not running"
        end

        animation_state.running = false
        minetest.chat_send_player(name, "[voxel_automata] Animation stopped")
        return true, "Animation stopped"
    end
})

-- /va_pull: Pull world state into automaton (recovery/debug command)
minetest.register_chatcommand("va_pull", {
    description = "Pull world nodes into automaton state (world → automaton sync)",
    func = function(name, param)
        if not global_state then
            return false, "No automaton state available"
        end

        -- Create buffer
        local buffer_size = grid_size * grid_size * grid_size
        local buffer = ffi.new("uint8_t[?]", buffer_size)

        -- Scan world region using VoxelManip
        local vm = VoxelManip()
        local world_min = viewport_anchor
        local world_max = {
            x = viewport_anchor.x + grid_size - 1,
            y = viewport_anchor.y + grid_size - 1,
            z = viewport_anchor.z + grid_size - 1
        }

        local emerged_min, emerged_max = vm:read_from_map(world_min, world_max)
        local data = vm:get_data()
        local area = VoxelArea:new({ MinEdge = emerged_min, MaxEdge = emerged_max })
        local cell_id = minetest.get_content_id("voxel_automata:cell")

        -- Fill buffer from world (z,y,x order)
        local offset = 0
        local synced_alive = 0
        for z = 0, grid_size - 1 do
            for y = 0, grid_size - 1 do
                for x = 0, grid_size - 1 do
                    local world_pos = {
                        x = viewport_anchor.x + x,
                        y = viewport_anchor.y + y,
                        z = viewport_anchor.z + z
                    }
                    local vi = area:indexp(world_pos)
                    local is_alive = (data[vi] == cell_id) and 1 or 0
                    buffer[offset] = is_alive
                    if is_alive == 1 then
                        synced_alive = synced_alive + 1
                    end
                    offset = offset + 1
                end
            end
        end

        -- Import to automaton
        local bytes_read = va.va_import_region(
            global_state,
            buffer,
            0, 0, 0,
            grid_size, grid_size, grid_size
        )

        if bytes_read == 0 then
            return false, "Failed to import region"
        end

        minetest.chat_send_player(name,
            string.format("[voxel_automata] Pulled %d alive cells from world into automaton", synced_alive))
        return true, "World → automaton pull complete"
    end
})

-- ============================================================================
-- Phase 8b: Field visualization and debugging commands
-- ============================================================================

-- /va_show_field: Manually render field for debugging
minetest.register_chatcommand("va_show_field", {
    description = "Render u32 field as grayscale blocks",
    func = function(name, param)
        if not global_step_controller then
            return false, "No StepController available"
        end

        local start_time = minetest.get_us_time()
        render_field_grayscale()
        local elapsed = (minetest.get_us_time() - start_time) / 1000

        local gen = va.va_sc_field_get_generation(global_step_controller)
        minetest.chat_send_player(name, string.format("[voxel_automata] Field rendered in %.2f ms (generation %d)",
            elapsed, tonumber(gen)))
        return true, "Field rendered"
    end
})

-- /va_field_info: Show StepController and field visualization status
minetest.register_chatcommand("va_field_info", {
    description = "Show StepController and field visualization status",
    func = function(name, param)
        if not global_step_controller then
            return false, "No StepController available"
        end

        local is_stepping = va.va_sc_is_stepping(global_step_controller)
        local generation = va.va_sc_field_get_generation(global_step_controller)

        -- Calculate total field mass
        local total_mass = 0
        for z = 0, 15 do
            for y = 0, 15 do
                for x = 0, 15 do
                    total_mass = total_mass + va.va_sc_field_get(global_step_controller, x, y, z)
                end
            end
        end

        -- Sample corner cell
        local corner_value = va.va_sc_field_get(global_step_controller, 0, 0, 0)
        local corner_grayscale = math.floor(corner_value / 16777216)

        minetest.chat_send_player(name, "[voxel_automata] Generation: " .. tonumber(generation))
        minetest.chat_send_player(name, "[voxel_automata] Currently stepping: " .. (is_stepping == 1 and "yes" or "no"))
        minetest.chat_send_player(name,
            "[voxel_automata] Lua field_stepping flag: " .. tostring(animation_state.field_stepping))
        minetest.chat_send_player(name, string.format("[voxel_automata] Total mass: %d", total_mass))
        minetest.chat_send_player(name, string.format("[voxel_automata] Corner cell (0,0,0): value=%d, grayscale=%d",
            corner_value, corner_grayscale))

        return true, "Info displayed"
    end
})

-- Cleanup on shutdown
minetest.register_on_shutdown(function()
    if global_state ~= nil then
        minetest.log("action", "[voxel_automata] Destroying state on shutdown")
        va.va_destroy(global_state)
        global_state = nil
    end
    if global_field ~= nil then
        minetest.log("action", "[voxel_automata] Destroying field on shutdown")
        va.va_destroy_field(global_field)
        global_field = nil
    end
    if global_step_controller ~= nil then
        minetest.log("action", "[voxel_automata] Destroying step controller on shutdown")
        va.va_destroy_step_controller(global_step_controller)
        global_step_controller = nil
    end
end)
