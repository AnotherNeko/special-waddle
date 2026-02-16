-- Voxel Automata: Performance Test (Phase 8a Track A)
--
-- Timing architecture: two decoupled systems
--   (A) Sim step rate  — controlled by tickrate_ms, how often a new generation starts
--   (B) Tick budget    — 4ms per frame, how much compute per Luanti tick
--
-- State machine:
--   WAITING  -> timer accumulates; when >= tickrate, begin_step -> STEPPING
--   STEPPING -> va_sc_tick 4ms/frame until done=1 -> RENDERING
--   RENDERING -> render once, record elapsed time -> WAITING

return function(M)
    local va = M.va

    M.perf_test_state = {
        active = false,
        phase = "waiting", -- "waiting" | "stepping" | "rendering"
        controller = nil,
        tick_count = 0,
        target_ticks = 60,
        frame_times = {},
        worst_frame_ms = 0,
        step_start_us = 0, -- when current step began (for elapsed measurement)
        timer = 0.0,       -- accumulates dtime during WAITING
        tickrate_ms = 1000,
        size_x = 0,
        size_y = 0,
        size_z = 0,
    }

    minetest.register_chatcommand("va_perf", {
        description =
        "Measure real frame times during field stepping. Usage: /va_perf <size_x> [size_y] [size_z] [tickrate_ms]",
        func = function(name, param)
            if M.perf_test_state.active then
                return false, "Performance test already in progress"
            end

            local parts = {}
            for part in param:gmatch("[^ ]+") do
                table.insert(parts, tonumber(part))
            end

            if #parts < 1 then
                return false, "Usage: /va_perf <size_x> [size_y] [size_z] [tickrate_ms]"
            end

            local size_x = parts[1]
            local size_y = parts[2] or parts[1]
            local size_z = parts[3] or parts[1]
            local tickrate_ms = parts[4] or 1000

            if size_x < 8 or size_x > 1024 or size_y < 8 or size_y > 1024 or size_z < 8 or size_z > 1024 then
                return false, "Dimensions must be between 8 and 1024"
            end

            if tickrate_ms < 10 or tickrate_ms > 10000 then
                return false, "Tickrate must be between 10 and 10000 ms"
            end

            local ctrl = va.va_create_step_controller(size_x, size_y, size_z, 3, 1)
            if ctrl == nil then
                return false, "Failed to create step controller"
            end

            -- Initialize with a noisy state (values large enough to be visible in grayscale)
            for z = 0, size_z - 1 do
                for y = 0, size_y - 1 do
                    for x = 0, size_x - 1 do
                        local hash = (x * 73856093) * ((y * 19349663) + 1) * ((z * 83492791) + 1)
                        hash = bit.band(hash, 0xFFFFFFFF)
                        local value = 0
                        if hash % 17 == 0 then
                            value = 4000000000
                        elseif hash % 13 == 0 then
                            value = 2000000000
                        end
                        if value > 0 then
                            va.va_sc_field_set(ctrl, x, y, z, value)
                        end
                    end
                end
            end

            local p = M.perf_test_state
            p.active = true
            p.phase = "waiting"
            p.controller = ctrl
            p.tick_count = 0
            p.target_ticks = 60
            p.frame_times = {}
            p.worst_frame_ms = 0
            p.step_start_us = 0
            p.timer = 0.0
            p.tickrate_ms = tickrate_ms
            p.size_x = size_x
            p.size_y = size_y
            p.size_z = size_z

            minetest.chat_send_player(name,
                string.format("[voxel_automata] Starting perf test: %dx%dx%d (tickrate: %d ms, %d-step observation)",
                    size_x, size_y, size_z, tickrate_ms, p.target_ticks))
            minetest.chat_send_player(name,
                string.format("[voxel_automata] Field will render at viewport_anchor: %s",
                    minetest.pos_to_string(M.viewport_anchor)))
            return true, "Performance test started"
        end
    })

    minetest.register_globalstep(function(dtime)
        local p = M.perf_test_state
        if not p.active or not p.controller then return end

        if p.phase == "waiting" then
            -- Accumulate time; start a new step when tickrate interval has passed
            p.timer = p.timer + dtime
            if p.timer >= p.tickrate_ms / 1000.0 then
                p.timer = p.timer - (p.tickrate_ms / 1000.0)
                p.phase = "stepping"
                p.step_start_us = minetest.get_us_time()
                va.va_sc_begin_step(p.controller)
            end
        elseif p.phase == "stepping" then
            -- Tick with small budget; stay in STEPPING until step completes
            local done = va.va_sc_tick(p.controller, 4000) -- 4ms budget
            if done == 1 then
                p.phase = "rendering"
            end
        elseif p.phase == "rendering" then
            -- Render once, record total elapsed time (stepping + rendering)
            -- NOTE: VoxelManip is likely the dominant bottleneck at large grid sizes.
            -- At 128^3 (~2M cells), render alone costs ~400-700ms per step.
            local render_count = M.render_field_grayscale_at(
                p.controller,
                M.viewport_anchor.x, M.viewport_anchor.y, M.viewport_anchor.z,
                p.size_x, p.size_y, p.size_z
            )

            local elapsed_ms = (minetest.get_us_time() - p.step_start_us) / 1000
            p.tick_count = p.tick_count + 1
            table.insert(p.frame_times, elapsed_ms)
            if elapsed_ms > p.worst_frame_ms then
                p.worst_frame_ms = elapsed_ms
            end

            minetest.log("action", string.format(
                "[voxel_automata] Perf test step %d/%d: %.2f ms total, %d nonzero cells",
                p.tick_count, p.target_ticks, elapsed_ms, render_count or 0))

            if p.tick_count >= p.target_ticks then
                -- Test complete: report results
                local total_ms = 0
                for _, ms in ipairs(p.frame_times) do total_ms = total_ms + ms end
                local avg_ms = total_ms / #p.frame_times
                local min_ms = math.min(unpack(p.frame_times))
                local gen = va.va_sc_field_get_generation(p.controller)

                local success = p.worst_frame_ms < 75 and avg_ms < 40

                minetest.log("action", string.format(
                    "[voxel_automata] PERF TEST COMPLETE (generation %d): %dx%dx%d, %d steps",
                    tonumber(gen), p.size_x, p.size_y, p.size_z, #p.frame_times))
                minetest.log("action", string.format(
                    "  Avg: %.2f ms, Worst: %.2f ms, Min: %.2f ms",
                    avg_ms, p.worst_frame_ms, min_ms))
                minetest.log("action", string.format(
                    "  Success criteria: %s (worst < 75ms, avg < 40ms)",
                    success and "PASS" or "FAIL"))

                va.va_destroy_step_controller(p.controller)
                p.active = false
                p.controller = nil
            else
                p.phase = "waiting"
            end
        end
    end)
end
