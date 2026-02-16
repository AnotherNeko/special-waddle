-- Voxel Automata: Animation System (Phase 5 / Phase 8a)

return function(M)
    local va = M.va

    M.animation_state = {
        running = false,
        interval = 1.0,         -- seconds between steps
        timer = 0.0,
        field_stepping = false, -- Phase 8a: track if StepController is mid-step
    }

    minetest.register_globalstep(function(dtime)
        if not M.global_state or not M.global_step_controller then
            return
        end

        -- Phase 8a: Handle ongoing incremental step (non-blocking work)
        if M.animation_state.field_stepping then
            local done = va.va_sc_tick(M.global_step_controller, 4000) -- 4ms budget per tick
            if done == 1 then
                M.animation_state.field_stepping = false
                local gen = va.va_sc_field_get_generation(M.global_step_controller)
                minetest.log("action", "[voxel_automata] Incremental step completed: generation " .. tonumber(gen))
                M.render_field_grayscale()
            end
            return -- Continue processing this step, don't start new work
        end

        if not M.animation_state.running then
            return
        end

        M.animation_state.timer = M.animation_state.timer + dtime

        if M.animation_state.timer >= M.animation_state.interval then
            M.animation_state.timer = 0

            -- Step the cellular automaton (blocking, Phase 3 behavior)
            va.va_step(M.global_state)

            -- Phase 8a: Begin new incremental field step (non-blocking)
            local result = va.va_sc_begin_step(M.global_step_controller)
            if result == 0 then
                M.animation_state.field_stepping = true
                -- Do first tick of work immediately (avoid one-frame delay)
                local done = va.va_sc_tick(M.global_step_controller, 4000)
                if done == 1 then
                    M.animation_state.field_stepping = false
                    local gen = va.va_sc_field_get_generation(M.global_step_controller)
                    minetest.log("action",
                        "[voxel_automata] Incremental step completed immediately: generation " .. tonumber(gen))
                    M.render_field_grayscale()
                end
            end

            -- Render the cellular automaton using VoxelManip at viewport anchor
            M.render_region_at_world_voxelmanip(
                0, 0, 0,
                M.grid_size, M.grid_size, M.grid_size,
                M.viewport_anchor.x, M.viewport_anchor.y, M.viewport_anchor.z
            )

            local gen = va.va_get_generation(M.global_state)
            minetest.log("action", "[voxel_automata] Animation step: CA generation " .. tonumber(gen))
        end
    end)
end
