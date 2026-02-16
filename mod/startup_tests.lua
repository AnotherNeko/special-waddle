-- Voxel Automata: Startup Runtime Tests
-- These tests run at mod load time and error() on failure (shows popup in Luanti menu)

return function(M)
    local va = M.va
    local ffi = M.ffi

    -- Helper: Assert with error popup
    local function test_assert(condition, test_name, details)
        if not condition then
            local msg = string.format("[voxel_automata] TEST FAILED: %s - %s", test_name, details or "")
            minetest.log("error", msg)
            error(msg)
        end
        minetest.log("action", string.format("[voxel_automata] PASS: %s", test_name))
    end

    -- Phase 1: FFI arithmetic
    local result = va.va_add(2, 3)
    test_assert(result == 5, "Phase 1: FFI arithmetic", string.format("expected 5, got %d", result))

    -- Phase 2: Handle lifecycle
    local state = va.va_create()
    test_assert(state ~= nil, "Phase 2: Create state", "va_create() returned nil")

    local generation = va.va_get_generation(state)
    test_assert(tonumber(generation) == 0, "Phase 2: Initial generation",
        string.format("expected 0, got %d", tonumber(generation)))

    M.global_state = state

    -- Phase 3: Grid + cells
    local grid_result = va.va_create_grid(state, 16, 16, 16)
    test_assert(grid_result == 0, "Phase 3: Create grid",
        string.format("create_grid failed with code %d", grid_result))

    -- Set cells in cross pattern
    va.va_set_cell(state, 8, 8, 8, 1) -- Center
    va.va_set_cell(state, 7, 8, 8, 1) -- Left
    va.va_set_cell(state, 9, 8, 8, 1) -- Right
    va.va_set_cell(state, 8, 7, 8, 1) -- Front
    va.va_set_cell(state, 8, 9, 8, 1) -- Back

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
    test_assert(alive_count == 5, "Phase 3: Cell count",
        string.format("expected 5 alive cells, got %d", alive_count))

    -- Phase 6: Field diffusion
    local field = va.va_create_field(16, 16, 16, 2)
    test_assert(field ~= nil, "Phase 6: Create field", "va_create_field() returned nil")

    va.va_field_set(field, 8, 8, 8, 1000000)
    local initial_value = va.va_field_get(field, 8, 8, 8)
    test_assert(initial_value == 1000000, "Phase 6: Field set/get",
        string.format("expected 1000000, got %d", initial_value))

    va.va_field_step(field)
    local center_after = va.va_field_get(field, 8, 8, 8)
    local neighbor_x = va.va_field_get(field, 7, 8, 8)

    test_assert(center_after < initial_value, "Phase 6: Diffusion center decreased",
        string.format("center should decrease, was %d, now %d", initial_value, center_after))
    test_assert(neighbor_x > 0, "Phase 6: Diffusion to neighbors",
        string.format("neighbors should have positive value, got %d", neighbor_x))

    M.global_field = field

    -- Phase 8a: StepController basic
    local ctrl = va.va_create_step_controller(16, 16, 16, 2, 1)
    test_assert(ctrl ~= nil, "Phase 8a: Create StepController", "va_create_step_controller() returned nil")

    va.va_sc_field_set(ctrl, 0, 0, 0, 3999995905)
    local sc_value = va.va_sc_field_get(ctrl, 0, 0, 0)
    test_assert(sc_value == 3999995905, "Phase 8a: StepController set/get",
        string.format("expected 3999995905, got %d", sc_value))

    -- Test blocking step
    local gen_before = va.va_sc_field_get_generation(ctrl)
    va.va_sc_step_blocking(ctrl)
    local gen_after = va.va_sc_field_get_generation(ctrl)
    test_assert(gen_after == gen_before + 1, "Phase 8a: Blocking step increments generation",
        string.format("expected gen %d, got %d", gen_before + 1, gen_after))

    M.global_step_controller = ctrl

    -- NEW: StepController incremental stepping
    local ctrl2 = va.va_create_step_controller(8, 8, 8, 2, 1)
    test_assert(ctrl2 ~= nil, "Phase 8a: Create test StepController", "returned nil")

    va.va_sc_field_set(ctrl2, 4, 4, 4, 1000000)

    local gen_before_inc = va.va_sc_field_get_generation(ctrl2)
    local begin_result = va.va_sc_begin_step(ctrl2)
    test_assert(begin_result == 0, "Phase 8a: begin_step success",
        string.format("expected 0, got %d", begin_result))

    local done = va.va_sc_tick(ctrl2, 100000000) -- Large budget to complete immediately
    test_assert(done == 1, "Phase 8a: Incremental step completes",
        string.format("expected done=1, got %d", done))

    local gen_after_inc = va.va_sc_field_get_generation(ctrl2)
    test_assert(gen_after_inc == gen_before_inc + 1, "Phase 8a: Incremental step increments generation",
        string.format("expected gen %d, got %d", gen_before_inc + 1, gen_after_inc))

    va.va_destroy_step_controller(ctrl2)

    -- NEW: Grayscale mapping verification
    local function grayscale_from_u32(value)
        return math.floor(value / 16777216) -- Divide by 2^24
    end

    test_assert(grayscale_from_u32(0) == 0, "Grayscale: zero maps to 0")
    test_assert(grayscale_from_u32(100000) == 0, "Grayscale: low value maps to 0")
    test_assert(grayscale_from_u32(16777216) == 1, "Grayscale: 2^24 maps to 1")
    test_assert(grayscale_from_u32(1000000000) == 59, "Grayscale: 1B maps to 59",
        string.format("expected 59, got %d", grayscale_from_u32(1000000000)))
    test_assert(grayscale_from_u32(4278190080) == 255, "Grayscale: high value maps to 255",
        string.format("expected 255, got %d", grayscale_from_u32(4278190080)))

    -- All tests passed! Register a callback to notify the first player
    M.tests_passed = true
    minetest.register_on_joinplayer(function(player)
        if M.tests_passed then
            minetest.chat_send_player(player:get_player_name(),
                "[voxel_automata] All startup tests passed ✓")
            M.tests_passed = false -- Only show once
        end
    end)

    minetest.log("action", "[voxel_automata] ========================================")
    minetest.log("action", "[voxel_automata] All startup tests PASSED")
    minetest.log("action", "[voxel_automata] ========================================")
end
