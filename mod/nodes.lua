-- Voxel Automata: Node Registrations

return function(M)
    local va = M.va
    local world_to_automaton = M.world_to_automaton
    local is_in_automaton_bounds = M.is_in_automaton_bounds

    -- Phase 4/5: Register node type for visualization with interaction callbacks
    minetest.register_node("voxel_automata:cell", {
        description = "Cellular Automata Cell",
        tiles = { "voxel_automata_cell.png" },
        walkable = false,
        sunlight_propagates = true,
        groups = { dig_immediate = 3, not_in_creative_inventory = 1 },

        -- Phase 5: Sync cell removal back to automaton
        on_dig = function(pos, node, digger)
            if M.global_state then
                local auto_pos = world_to_automaton(pos)
                if is_in_automaton_bounds(auto_pos) then
                    va.va_set_cell(M.global_state, auto_pos.x, auto_pos.y, auto_pos.z, 0)
                    minetest.log("action", "[voxel_automata] Cell dug at automaton " .. minetest.pos_to_string(auto_pos))
                end
            end
            minetest.node_dig(pos, node, digger)
        end,
    })

    -- Phase 5: Sync cell placement back to automaton
    minetest.register_on_placenode(function(pos, newnode, placer, oldnode, itemstack, pointed_thing)
        if newnode.name == "voxel_automata:cell" and M.global_state then
            local auto_pos = world_to_automaton(pos)
            if is_in_automaton_bounds(auto_pos) then
                va.va_set_cell(M.global_state, auto_pos.x, auto_pos.y, auto_pos.z, 1)
                minetest.log("action", "[voxel_automata] Cell placed at automaton " .. minetest.pos_to_string(auto_pos))
            end
        end
    end)

    -- Phase 8b: Register 256 grayscale nodes for u32 field visualization
    -- Mapping: u32 value (0 to 4,294,967,295) → grayscale (0 to 255)
    -- Formula: grayscale = math.floor(value / 16777216)  -- divide by 2^24

    for i = 0, 255 do
        local brightness = i / 255.0
        local node_name = string.format("voxel_automata:mass_%03d", i)

        minetest.register_node(node_name, {
            description = string.format("Mass Field (level %d/255)", i),
            tiles = { {
                name = "voxel_automata_grayscale.png",
                color = { r = brightness * 255, g = brightness * 255, b = brightness * 255 },
            } },
            paramtype = "light",
            light_source = 0,
            sunlight_propagates = true,
            walkable = false,
            pointable = false,
            buildable_to = true,
            groups = { not_in_creative_inventory = 1 },
        })
    end

    minetest.log("action", "[voxel_automata] Registered 256 grayscale mass nodes")
end
