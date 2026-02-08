-- Voxel Automata: 3D Cellular Automata for Luanti
-- Phase 1: FFI Bridge Proof

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
    int32_t va_add(int32_t a, int32_t b);
]]

-- Load the Rust library
local lib_path = modpath .. "/lib/libvoxel_automata.so"
local va = ffi.load(lib_path)

-- Phase 1 test: Call va_add and print result
local a = 2
local b = 3
local result = va.va_add(a, b)

minetest.log("action", "[voxel_automata] Phase 1: " .. a .. " + " .. b .. " = " .. result)
minetest.log("action", "[voxel_automata] Loaded successfully!")

-- Send message when first player joins
local phase1_shown = false
minetest.register_on_joinplayer(function(player)
    if not phase1_shown then
        minetest.after(0.1, function()
            minetest.chat_send_all("[voxel_automata] Phase 1: " .. a .. " + " .. b .. " = " .. result)
        end)
        phase1_shown = true
    end
end)
