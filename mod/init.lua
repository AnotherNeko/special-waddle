-- Voxel Automata: 3D Cellular Automata for Luanti
-- Phase 3: Small Grid + Step

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
]]

-- Load the Rust library
local lib_path = modpath .. "/lib/libvoxel_automata.so"
local va = ffi.load(lib_path)

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

-- Step the automaton once
va.va_step(state)
local generation_after_step = va.va_get_generation(state)
minetest.log("action", "[voxel_automata] Phase 3: After step, generation = " .. tonumber(generation_after_step))

-- Count alive cells after step
local alive_count_after = 0
for z = 0, 15 do
    for y = 0, 15 do
        for x = 0, 15 do
            if va.va_get_cell(state, x, y, z) == 1 then
                alive_count_after = alive_count_after + 1
            end
        end
    end
end
minetest.log("action", "[voxel_automata] Phase 3: Alive count after step = " .. alive_count_after)
minetest.log("action", "[voxel_automata] Loaded successfully!")

-- Store state for cleanup
local global_state = state

-- Send message when first player joins
local test_shown = false
minetest.register_on_joinplayer(function(player)
    if not test_shown then
        minetest.after(0.1, function()
            minetest.chat_send_all("[voxel_automata] Phase 1: " .. a .. " + " .. b .. " = " .. result)
            minetest.chat_send_all("[voxel_automata] Phase 2: generation = " .. tonumber(generation))
            minetest.chat_send_all("[voxel_automata] Phase 3: Initial alive = " .. alive_count .. ", After step = " .. alive_count_after)
        end)
        test_shown = true
    end
end)

-- Cleanup on shutdown
minetest.register_on_shutdown(function()
    if global_state ~= nil then
        minetest.log("action", "[voxel_automata] Destroying state on shutdown")
        va.va_destroy(global_state)
        global_state = nil
    end
end)
