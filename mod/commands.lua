-- Voxel Automata: Chat Commands

return function(M)
    local va = M.va
    local ffi = M.ffi

    minetest.register_chatcommand("va_info", {
        description = "Show automaton statistics (generation, alive cells, grid size, viewport anchor)",
        func = function(name, param)
            if not M.global_state then
                return false, "No automaton state available"
            end

            local generation = va.va_get_generation(M.global_state)
            local alive_count = 0
            for z = 0, M.grid_size - 1 do
                for y = 0, M.grid_size - 1 do
                    for x = 0, M.grid_size - 1 do
                        if va.va_get_cell(M.global_state, x, y, z) == 1 then
                            alive_count = alive_count + 1
                        end
                    end
                end
            end

            minetest.chat_send_player(name, "[voxel_automata] Generation: " .. tonumber(generation))
            minetest.chat_send_player(name,
                "[voxel_automata] Alive cells: " .. alive_count .. " / " .. (M.grid_size ^ 3))
            minetest.chat_send_player(name,
                string.format("[voxel_automata] Grid size: %dx%dx%d", M.grid_size, M.grid_size, M.grid_size))
            minetest.chat_send_player(name,
                "[voxel_automata] Viewport anchor: " .. minetest.pos_to_string(M.viewport_anchor))
            minetest.chat_send_player(name,
                "[voxel_automata] Animation: " .. (M.animation_state.running and "running" or "stopped"))
            return true, "Info displayed"
        end
    })

    minetest.register_chatcommand("va_step", {
        description = "Step the automaton forward by N generations. Usage: /va_step [count]",
        func = function(name, param)
            if not M.global_state then
                return false, "No automaton state available"
            end

            local count = tonumber(param) or 1
            if count < 1 or count > 100 then
                return false, "Count must be between 1 and 100"
            end

            for i = 1, count do
                va.va_step(M.global_state)
            end

            local generation = va.va_get_generation(M.global_state)
            return true, "Stepped " .. count .. " generation(s). Now at generation " .. tonumber(generation)
        end
    })

    minetest.register_chatcommand("va_show", {
        description = "Render automaton grid at world position. Usage: /va_show [x] [y] [z]",
        func = function(name, param)
            if not M.global_state then
                return false, "No automaton state available"
            end

            local world_x, world_y, world_z = param:match("([^ ]+) ([^ ]+) ([^ ]+)")

            if not world_x or not world_y or not world_z then
                local player = minetest.get_player_by_name(name)
                if not player then return false, "Player not found" end
                local pos = player:get_pos()
                world_x = math.floor(pos.x / 16) * 16
                world_y = math.floor(pos.y / 16) * 16
                world_z = math.floor(pos.z / 16) * 16
            else
                world_x = tonumber(world_x)
                world_y = tonumber(world_y)
                world_z = tonumber(world_z)
            end

            M.viewport_anchor.x = world_x
            M.viewport_anchor.y = world_y
            M.viewport_anchor.z = world_z

            local start_time = minetest.get_us_time()
            M.render_region_at_world_voxelmanip(
                0, 0, 0, M.grid_size, M.grid_size, M.grid_size,
                world_x, world_y, world_z
            )
            local elapsed = (minetest.get_us_time() - start_time) / 1000

            minetest.chat_send_player(name,
                string.format("[voxel_automata] Rendered in %.2f ms at %s",
                    elapsed, minetest.pos_to_string(M.viewport_anchor)))
            return true, "Automaton rendered"
        end
    })

    minetest.register_chatcommand("va_animate", {
        description = "Start automatic stepping and rendering. Usage: /va_animate [interval_ms]",
        func = function(name, param)
            if not M.global_state then
                return false, "No automaton state available"
            end

            local interval_ms = tonumber(param) or 1000
            if interval_ms < 100 or interval_ms > 10000 then
                return false, "Interval must be between 100 and 10000 milliseconds"
            end

            M.animation_state.running = true
            M.animation_state.interval = interval_ms / 1000.0
            M.animation_state.timer = 0

            minetest.chat_send_player(name,
                string.format("[voxel_automata] Animation started (interval: %d ms)", interval_ms))
            return true, "Animation started"
        end
    })

    minetest.register_chatcommand("va_stop", {
        description = "Stop automatic animation and/or performance test",
        func = function(name, param)
            local animation_running = M.animation_state.running
            local perf_running = M.perf_test_state.active

            if not animation_running and not perf_running then
                return false, "Nothing is running"
            end

            M.animation_state.running = false
            M.perf_test_state.active = false

            local msg = ""
            if animation_running then msg = msg .. "Animation " end
            if perf_running then msg = msg .. (animation_running and "& Perf test " or "Perf test ") end
            msg = msg .. "stopped"

            minetest.chat_send_player(name, "[voxel_automata] " .. msg)
            return true, msg
        end
    })

    minetest.register_chatcommand("va_pull", {
        description = "Pull world nodes into automaton state (world → automaton sync)",
        func = function(name, param)
            if not M.global_state then
                return false, "No automaton state available"
            end

            local buffer_size = M.grid_size ^ 3
            local buffer = ffi.new("uint8_t[?]", buffer_size)

            local vm = VoxelManip()
            local world_min = M.viewport_anchor
            local world_max = {
                x = M.viewport_anchor.x + M.grid_size - 1,
                y = M.viewport_anchor.y + M.grid_size - 1,
                z = M.viewport_anchor.z + M.grid_size - 1
            }

            local emerged_min, emerged_max = vm:read_from_map(world_min, world_max)
            local data = vm:get_data()
            local area = VoxelArea:new({ MinEdge = emerged_min, MaxEdge = emerged_max })
            local cell_id = minetest.get_content_id("voxel_automata:cell")

            local offset = 0
            local synced_alive = 0
            for z = 0, M.grid_size - 1 do
                for y = 0, M.grid_size - 1 do
                    for x = 0, M.grid_size - 1 do
                        local vi = area:indexp({
                            x = M.viewport_anchor.x + x,
                            y = M.viewport_anchor.y + y,
                            z = M.viewport_anchor.z + z
                        })
                        local is_alive = (data[vi] == cell_id) and 1 or 0
                        buffer[offset] = is_alive
                        if is_alive == 1 then synced_alive = synced_alive + 1 end
                        offset = offset + 1
                    end
                end
            end

            local bytes_read = va.va_import_region(
                M.global_state, buffer,
                0, 0, 0, M.grid_size, M.grid_size, M.grid_size
            )

            if bytes_read == 0 then
                return false, "Failed to import region"
            end

            minetest.chat_send_player(name,
                string.format("[voxel_automata] Pulled %d alive cells from world into automaton", synced_alive))
            return true, "World → automaton pull complete"
        end
    })

    minetest.register_chatcommand("va_show_field", {
        description = "Render u32 field as grayscale blocks",
        func = function(name, param)
            if not M.global_step_controller then
                return false, "No StepController available"
            end

            local start_time = minetest.get_us_time()
            M.render_field_grayscale()
            local elapsed = (minetest.get_us_time() - start_time) / 1000

            local gen = va.va_sc_field_get_generation(M.global_step_controller)
            minetest.chat_send_player(name,
                string.format("[voxel_automata] Field rendered in %.2f ms (generation %d)",
                    elapsed, tonumber(gen)))
            return true, "Field rendered"
        end
    })

    minetest.register_chatcommand("va_field_info", {
        description = "Show StepController and field visualization status",
        func = function(name, param)
            if not M.global_step_controller then
                return false, "No StepController available"
            end

            local is_stepping = va.va_sc_is_stepping(M.global_step_controller)
            local generation = va.va_sc_field_get_generation(M.global_step_controller)

            local total_mass = 0
            for z = 0, 15 do
                for y = 0, 15 do
                    for x = 0, 15 do
                        total_mass = total_mass + va.va_sc_field_get(M.global_step_controller, x, y, z)
                    end
                end
            end

            local corner_value = va.va_sc_field_get(M.global_step_controller, 0, 0, 0)
            local corner_grayscale = math.floor(corner_value / 16777216)

            minetest.chat_send_player(name, "[voxel_automata] Generation: " .. tonumber(generation))
            minetest.chat_send_player(name,
                "[voxel_automata] Currently stepping: " .. (is_stepping == 1 and "yes" or "no"))
            minetest.chat_send_player(name,
                "[voxel_automata] Lua field_stepping flag: " .. tostring(M.animation_state.field_stepping))
            minetest.chat_send_player(name, string.format("[voxel_automata] Total mass: %d", total_mass))
            minetest.chat_send_player(name,
                string.format("[voxel_automata] Corner cell (0,0,0): value=%d, grayscale=%d",
                    corner_value, corner_grayscale))
            return true, "Info displayed"
        end
    })
end
