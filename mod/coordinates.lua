-- Voxel Automata: Coordinate Helpers

return function(M)
    M.viewport_anchor = { x = 0, y = 0, z = 0 }
    M.grid_size = 16

    function M.world_to_automaton(world_pos)
        return {
            x = world_pos.x - M.viewport_anchor.x,
            y = world_pos.y - M.viewport_anchor.y,
            z = world_pos.z - M.viewport_anchor.z
        }
    end

    function M.automaton_to_world(auto_pos)
        return {
            x = auto_pos.x + M.viewport_anchor.x,
            y = auto_pos.y + M.viewport_anchor.y,
            z = auto_pos.z + M.viewport_anchor.z
        }
    end

    function M.is_in_automaton_bounds(auto_pos)
        return auto_pos.x >= 0 and auto_pos.x < M.grid_size
            and auto_pos.y >= 0 and auto_pos.y < M.grid_size
            and auto_pos.z >= 0 and auto_pos.z < M.grid_size
    end
end
