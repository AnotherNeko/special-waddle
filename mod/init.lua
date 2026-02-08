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
ffi.cdef[[
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
]]

-- Load the Rust library
local lib_path = modpath .. "/lib/libvoxel_automata.so"
local va = ffi.load(lib_path)

-- Global state holder (initialized later)
local global_state = nil

-- Phase 4: Register node type for visualization
minetest.register_node("voxel_automata:cell", {
    description = "Cellular Automata Cell",
    tiles = {"voxel_automata_cell.png"},
    walkable = false,
    sunlight_propagates = true,
    groups = {not_in_creative_inventory = 1},
})

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
                    minetest.set_node({x = x, y = y, z = z}, {name = "voxel_automata:cell"})
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
                    {x = world_x + (x - min_x), y = world_y + (y - min_y), z = world_z + (z - min_z)},
                    {name = node_name}
                )
                if cell_state == 1 then
                    placed_count = placed_count + 1
                end
                offset = offset + 1
            end
        end
    end

    minetest.log("action", "[voxel_automata] Placed " .. placed_count .. " nodes at world (" .. world_x .. "," .. world_y .. "," .. world_z .. ")")
end

-- VoxelManip variant (for Phase 6+ optimization when timing issues are resolved)
--[[
local function render_region_voxelmanip(min_x, min_y, min_z, max_x, max_y, max_z)
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

    -- Create VoxelManip for bulk writing
    local vm = minetest.get_voxel_manipulator()
    local emerged_min, emerged_max = vm:read_from_map(
        {x = min_x, y = min_y, z = min_z},
        {x = max_x - 1, y = max_y - 1, z = max_z - 1}
    )

    local data = vm:get_data()
    local area = VoxelArea:new({MinEdge = emerged_min, MaxEdge = emerged_max})

    local node_id = minetest.get_content_id("voxel_automata:cell")
    local air_id = minetest.get_content_id("air")

    -- Fill data array from buffer
    local offset = 0
    for z = min_z, max_z - 1 do
        for y = min_y, max_y - 1 do
            for x = min_x, max_x - 1 do
                local cell_state = buffer[offset]
                local vi = area:indexp({x = x, y = y, z = z})
                if cell_state == 1 then
                    data[vi] = node_id
                else
                    data[vi] = air_id
                end
                offset = offset + 1
            end
        end
    end

    -- Write back to map
    vm:set_data(data)
    vm:write_to_map(true)
    vm:update_map()
end
]]

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
va.va_set_cell(state, 8, 8, 8, 1)  -- Center
va.va_set_cell(state, 7, 8, 8, 1)  -- Left
va.va_set_cell(state, 9, 8, 8, 1)  -- Right
va.va_set_cell(state, 8, 7, 8, 1)  -- Front
va.va_set_cell(state, 8, 9, 8, 1)  -- Back
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

minetest.log("action", "[voxel_automata] Loaded successfully!")

-- Register chat commands for interaction
minetest.register_chatcommand("ca_step", {
    description = "Step the automaton forward by N generations. Usage: /ca_step [count]",
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

minetest.register_chatcommand("ca_test", {
    description = "Test voxel automata: print phase results",
    func = function(name, param)
        minetest.chat_send_player(name, "[voxel_automata] Phase 1: " .. a .. " + " .. b .. " = " .. result)
        minetest.chat_send_player(name, "[voxel_automata] Phase 2: generation = " .. tonumber(generation))
        minetest.chat_send_player(name, "[voxel_automata] Phase 3: Initial alive = " .. alive_count .. ", After step = " .. alive_count_after)
        return true, "Test results printed"
    end
})

minetest.register_chatcommand("ca_render", {
    description = "Render automata grid at world position. Usage: /ca_render [world_x] [world_y] [world_z]",
    func = function(name, param)
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

        -- Extract from automaton grid (0-15) and place at world coordinates
        minetest.log("action", "[voxel_automata] Extracting from grid (0-15) and placing at world (" .. world_x .. "," .. world_y .. "," .. world_z .. ")")
        render_region_at_world(0, 0, 0, 16, 16, 16, world_x, world_y, world_z)

        return true, "Rendered automata grid at world (" .. world_x .. "," .. world_y .. "," .. world_z .. ")"
    end
})

-- Cleanup on shutdown
minetest.register_on_shutdown(function()
    if global_state ~= nil then
        minetest.log("action", "[voxel_automata] Destroying state on shutdown")
        va.va_destroy(global_state)
        global_state = nil
    end
end)
