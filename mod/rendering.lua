-- Voxel Automata: Rendering Functions

return function(M)
    local va = M.va
    local ffi = M.ffi

    -- Render CA region using individual node placement (simple, slower)
    function M.render_region(min_x, min_y, min_z, max_x, max_y, max_z)
        if not M.global_state then
            minetest.log("warning", "[voxel_automata] Cannot render: global_state is nil")
            return
        end

        local width = max_x - min_x
        local height = max_y - min_y
        local depth = max_z - min_z
        local buffer_size = width * height * depth
        local buffer = ffi.new("uint8_t[?]", buffer_size)

        local bytes_written = va.va_extract_region(
            M.global_state, buffer,
            min_x, min_y, min_z, max_x, max_y, max_z
        )

        if bytes_written == 0 then
            minetest.log("warning", "[voxel_automata] Extract region returned 0 bytes")
            return
        end

        local offset = 0
        local placed_count = 0
        for z = min_z, max_z - 1 do
            for y = min_y, max_y - 1 do
                for x = min_x, max_x - 1 do
                    local cell_state = buffer[offset]
                    if cell_state == 1 then
                        minetest.set_node({ x = x, y = y, z = z }, { name = "voxel_automata:cell" })
                        placed_count = placed_count + 1
                    end
                    offset = offset + 1
                end
            end
        end

        minetest.log("action", "[voxel_automata] Placed " .. placed_count .. " nodes")
    end

    -- Render CA region at world position using individual node placement
    function M.render_region_at_world(min_x, min_y, min_z, max_x, max_y, max_z, world_x, world_y, world_z)
        if not M.global_state then
            minetest.log("warning", "[voxel_automata] Cannot render: global_state is nil")
            return
        end

        local width = max_x - min_x
        local height = max_y - min_y
        local depth = max_z - min_z
        local buffer_size = width * height * depth
        local buffer = ffi.new("uint8_t[?]", buffer_size)

        local bytes_written = va.va_extract_region(
            M.global_state, buffer,
            min_x, min_y, min_z, max_x, max_y, max_z
        )

        if bytes_written == 0 then
            minetest.log("warning", "[voxel_automata] Extract region returned 0 bytes")
            return
        end

        local offset = 0
        local placed_count = 0
        for z = min_z, max_z - 1 do
            for y = min_y, max_y - 1 do
                for x = min_x, max_x - 1 do
                    local cell_state = buffer[offset]
                    local node_name = cell_state == 1 and "voxel_automata:cell" or "air"
                    minetest.set_node(
                        { x = world_x + (x - min_x), y = world_y + (y - min_y), z = world_z + (z - min_z) },
                        { name = node_name }
                    )
                    if cell_state == 1 then placed_count = placed_count + 1 end
                    offset = offset + 1
                end
            end
        end

        minetest.log("action", string.format("[voxel_automata] Placed %d nodes at world (%d,%d,%d)",
            placed_count, world_x, world_y, world_z))
    end

    -- Render CA region at world position using VoxelManip (bulk, faster)
    function M.render_region_at_world_voxelmanip(min_x, min_y, min_z, max_x, max_y, max_z, world_x, world_y, world_z)
        if not M.global_state then
            minetest.log("warning", "[voxel_automata] Cannot render: global_state is nil")
            return
        end

        local width = max_x - min_x
        local height = max_y - min_y
        local depth = max_z - min_z
        local buffer_size = width * height * depth
        local buffer = ffi.new("uint8_t[?]", buffer_size)

        local bytes_written = va.va_extract_region(
            M.global_state, buffer,
            min_x, min_y, min_z, max_x, max_y, max_z
        )

        if bytes_written == 0 then
            minetest.log("warning", "[voxel_automata] Extract region returned 0 bytes")
            return
        end

        local world_min = { x = world_x, y = world_y, z = world_z }
        local world_max = { x = world_x + width - 1, y = world_y + height - 1, z = world_z + depth - 1 }

        local vm = VoxelManip()
        local emerged_min, emerged_max = vm:read_from_map(world_min, world_max)
        local data = vm:get_data()
        local area = VoxelArea:new({ MinEdge = emerged_min, MaxEdge = emerged_max })

        local node_id = minetest.get_content_id("voxel_automata:cell")
        local air_id = minetest.get_content_id("air")

        local offset = 0
        local placed_count = 0
        for z = min_z, max_z - 1 do
            for y = min_y, max_y - 1 do
                for x = min_x, max_x - 1 do
                    local cell_state = buffer[offset]
                    local world_pos = {
                        x = world_x + (x - min_x),
                        y = world_y + (y - min_y),
                        z = world_z + (z - min_z)
                    }
                    local vi = area:indexp(world_pos)
                    data[vi] = cell_state == 1 and node_id or air_id
                    if cell_state == 1 then placed_count = placed_count + 1 end
                    offset = offset + 1
                end
            end
        end

        vm:set_data(data)
        vm:write_to_map()
        vm:update_map()

        minetest.log("action", string.format("[voxel_automata] VoxelManip: Placed %d nodes at world (%d,%d,%d)",
            placed_count, world_x, world_y, world_z))
    end

    -- Render global StepController field as grayscale at field_anchor (y+50)
    function M.render_field_grayscale()
        if not M.global_step_controller then return end

        local field_anchor = {
            x = M.viewport_anchor.x,
            y = M.viewport_anchor.y + 50,
            z = M.viewport_anchor.z
        }
        local field_size = 16

        local world_min = field_anchor
        local world_max = {
            x = field_anchor.x + field_size - 1,
            y = field_anchor.y + field_size - 1,
            z = field_anchor.z + field_size - 1
        }

        local vm = VoxelManip()
        local emerged_min, emerged_max = vm:read_from_map(world_min, world_max)
        local data = vm:get_data()
        local area = VoxelArea:new({ MinEdge = emerged_min, MaxEdge = emerged_max })

        local grayscale_ids = {}
        for i = 0, 255 do
            grayscale_ids[i] = minetest.get_content_id(string.format("voxel_automata:mass_%03d", i))
        end
        local air_id = minetest.get_content_id("air")

        local nonzero_count = 0
        for z = 0, field_size - 1 do
            for y = 0, field_size - 1 do
                for x = 0, field_size - 1 do
                    local value = va.va_sc_field_get(M.global_step_controller, x, y, z)
                    local grayscale = math.floor(value / 16777216)
                    if grayscale > 255 then grayscale = 255 end
                    local vi = area:indexp({
                        x = field_anchor.x + x,
                        y = field_anchor.y + y,
                        z = field_anchor.z + z
                    })
                    data[vi] = grayscale > 0 and grayscale_ids[grayscale] or air_id
                    if value > 0 then nonzero_count = nonzero_count + 1 end
                end
            end
        end

        vm:set_data(data)
        vm:write_to_map()
        vm:update_map()

        minetest.log("action", string.format("[voxel_automata] Field rendered: %d/%d cells nonzero",
            nonzero_count, field_size * field_size * field_size))
    end

    -- Render arbitrary StepController field as grayscale at world position
    function M.render_field_grayscale_at(controller, world_x, world_y, world_z, size_x, size_y, size_z)
        if not controller then return end

        local world_min = { x = world_x, y = world_y, z = world_z }
        local world_max = {
            x = world_x + size_x - 1,
            y = world_y + size_y - 1,
            z = world_z + size_z - 1
        }

        local vm = VoxelManip()
        local emerged_min, emerged_max = vm:read_from_map(world_min, world_max)
        local data = vm:get_data()
        local area = VoxelArea:new({ MinEdge = emerged_min, MaxEdge = emerged_max })

        local grayscale_ids = {}
        for i = 0, 255 do
            grayscale_ids[i] = minetest.get_content_id(string.format("voxel_automata:mass_%03d", i))
        end
        local air_id = minetest.get_content_id("air")

        local nonzero_count = 0
        for z = 0, size_z - 1 do
            for y = 0, size_y - 1 do
                for x = 0, size_x - 1 do
                    local value = va.va_sc_field_get(controller, x, y, z)
                    local grayscale = math.floor(value / 16777216)
                    if grayscale > 255 then grayscale = 255 end
                    local vi = area:indexp({ x = world_x + x, y = world_y + y, z = world_z + z })
                    data[vi] = grayscale > 0 and grayscale_ids[grayscale] or air_id
                    if value > 0 then nonzero_count = nonzero_count + 1 end
                end
            end
        end

        vm:set_data(data)
        vm:write_to_map()
        vm:update_map()

        return nonzero_count
    end
end
