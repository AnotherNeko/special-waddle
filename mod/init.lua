-- Voxel Automata: 3D Cellular Automata for Luanti
-- Phase 2: Opaque Handle Lifecycle

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
