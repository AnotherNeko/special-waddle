-- Voxel Automata: 3D Cellular Automata for Luanti
-- Entry point: loads FFI, shared state, and all submodules

local modname = minetest.get_current_modname()
local modpath = minetest.get_modpath(modname)

-- Get insecure environment to access FFI
local ie = minetest.request_insecure_environment()
if not ie then
    error("voxel_automata requires access to insecure environment. Add it to secure.trusted_mods in minetest.conf")
end

local ffi = ie.require("ffi")

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

local va = ffi.load(modpath .. "/lib/libvoxel_automata.so")

-- Shared state passed to all submodules
local M = {
    va                     = va,
    ffi                    = ffi,
    -- Populated by startup_tests.lua:
    global_state           = nil,
    global_field           = nil,
    global_step_controller = nil,
    tests_passed           = false,
    -- Populated by coordinates.lua:
    viewport_anchor        = nil,
    grid_size              = nil,
    -- Populated by animation.lua:
    animation_state        = nil,
    -- Populated by perf.lua:
    perf_test_state        = nil,
}

-- Load submodules in dependency order
dofile(modpath .. "/coordinates.lua")(M)
dofile(modpath .. "/rendering.lua")(M)
dofile(modpath .. "/startup_tests.lua")(M)
dofile(modpath .. "/nodes.lua")(M)
dofile(modpath .. "/animation.lua")(M)
dofile(modpath .. "/perf.lua")(M)
dofile(modpath .. "/commands.lua")(M)

minetest.log("action", "[voxel_automata] Loaded successfully!")

-- Cleanup on shutdown
minetest.register_on_shutdown(function()
    -- Stop active systems first to prevent globalstep from running during shutdown
    if M.animation_state then
        M.animation_state.running = false
    end
    if M.perf_test_state and M.perf_test_state.active then
        minetest.log("action", "[voxel_automata] Aborting active perf test on shutdown")
        M.perf_test_state.active = false
    end

    if M.global_state ~= nil then
        minetest.log("action", "[voxel_automata] Destroying state on shutdown")
        va.va_destroy(M.global_state)
        M.global_state = nil
    end
    if M.global_field ~= nil then
        minetest.log("action", "[voxel_automata] Destroying field on shutdown")
        va.va_destroy_field(M.global_field)
        M.global_field = nil
    end
    if M.global_step_controller ~= nil then
        minetest.log("action", "[voxel_automata] Destroying step controller on shutdown")
        va.va_destroy_step_controller(M.global_step_controller)
        M.global_step_controller = nil
    end
    if M.perf_test_state and M.perf_test_state.controller ~= nil then
        minetest.log("action", "[voxel_automata] Destroying perf test controller on shutdown")
        va.va_destroy_step_controller(M.perf_test_state.controller)
        M.perf_test_state.controller = nil
    end
end)
