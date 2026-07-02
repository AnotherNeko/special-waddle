-- Voxel Automata: Animation System (Phase 5 / Phase 8a / Phase 9c)

return function(M)
    local va = M.va

    M.animation_state = {
        running = false,
        interval = 1.0,         -- seconds between steps
        timer = 0.0,
        render_every = 16,      -- render every N global ticks
        render_countdown = 0,
    }

    minetest.register_globalstep(function(dtime)
        if not M.global_state or not M.global_step_controller then
            return
        end

        if not M.animation_state.running then
            return
        end

        M.animation_state.timer = M.animation_state.timer + dtime

        if M.animation_state.timer >= M.animation_state.interval then
            M.animation_state.timer = 0

            -- Phase 9c: Drive one global tick of cadence-aware stepping
            local zones_stepped = va.va_sc_cadence_step(M.global_step_controller)

            -- Poll any pending merge
            if M.pending_merge then
                local pm = M.pending_merge
                local result = va.va_sc_cadence_merge_poll(
                    M.global_step_controller,
                    pm.null_x, pm.null_y, pm.null_z,
                    pm.alt_x,  pm.alt_y,  pm.alt_z)
                if result == 1 then
                    M.pending_merge = nil
                    minetest.log("action", "[voxel_automata] Merge complete")
                    M.render_cadence_zones()
                end
            end

            -- Re-render field and cadence overlay every N ticks
            M.animation_state.render_countdown = (M.animation_state.render_countdown or 0) - 1
            if M.animation_state.render_countdown <= 0 then
                M.animation_state.render_countdown = M.animation_state.render_every
                M.render_field_grayscale()
                M.render_cadence_zones()
            end

            local gen = va.va_sc_field_get_generation(M.global_step_controller)
            minetest.log("action", "[voxel_automata] Animation step: generation " .. tonumber(gen))
        end
    end)
end
