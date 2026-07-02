-- Voxel Automata: Cadence Zones + SeamPlane Node (Phase 9c)

return function(M)
    local va = M.va

    M.pending_merge = nil

    -- Compute cadence from distance to field origin
    local function compute_cadence_for_halfspace(axis, seam_coord, side)
        -- Assign cadence based on distance from seam
        if side == "lo" then
            return math.max(1, math.min(32, math.floor(seam_coord / 2) + 1))
        else
            return math.max(1, math.min(32, math.floor((16 - seam_coord) / 2) + 1))
        end
    end

    local MAX_LEAVES = 4096

    -- Find the cadence-tree leaf nearest to (fcx,fcy,fcz) whose extent straddles
    -- seam_coord on the given axis, and return a point inside it (closest point
    -- on the leaf's bounding box to the seamplane's position on the other two
    -- axes). Returns nil if no leaf straddles the seam (e.g. seamplane sits
    -- outside the field entirely).
    local function find_seam_point(axis, seam_coord, fcx, fcy, fcz)
        if not M.global_step_controller then return nil end

        local buf = M.ffi.new("int16_t[?]", MAX_LEAVES * 7)
        local n = va.va_sc_cadence_leaves(M.global_step_controller, buf, MAX_LEAVES)

        local best_dist2, best_point, best_mn, best_mx = nil, nil, nil, nil
        for i = 0, n - 1 do
            local o = i * 7
            local mn = { buf[o], buf[o + 1], buf[o + 2] }
            local mx = { buf[o + 3], buf[o + 4], buf[o + 5] }

            minetest.log("action", string.format(
                "[voxel_automata] leaf candidate %d: mn=(%d,%d,%d) mx=(%d,%d,%d) cadence=%d",
                i, mn[1], mn[2], mn[3], mx[1], mx[2], mx[3], buf[o + 6]))

            if seam_coord >= mn[axis + 1] and seam_coord < mx[axis + 1] then
                local point = { fcx, fcy, fcz }
                point[axis + 1] = seam_coord

                local dist2 = 0
                for a = 0, 2 do
                    if a ~= axis then
                        local c = math.max(mn[a + 1], math.min(mx[a + 1] - 1, point[a + 1]))
                        local d = point[a + 1] - c
                        dist2 = dist2 + d * d
                        point[a + 1] = c
                    end
                end

                if best_dist2 == nil or dist2 < best_dist2 then
                    best_dist2 = dist2
                    best_point = point
                    best_mn = mn
                    best_mx = mx
                end
            end
        end

        if best_point then
            minetest.log("action", string.format(
                "[voxel_automata] selected leaf: mn=(%d,%d,%d) mx=(%d,%d,%d) -> point=(%d,%d,%d) dist2=%d",
                best_mn[1], best_mn[2], best_mn[3], best_mx[1], best_mx[2], best_mx[3],
                best_point[1], best_point[2], best_point[3], best_dist2))
        else
            minetest.log("action", string.format(
                "[voxel_automata] no leaf straddles axis=%d seam_coord=%d among %d leaves",
                axis, seam_coord, n))
        end

        return best_point
    end

    -- Render the cadence zone overlay as colored voxels
    function M.render_cadence_zones()
        if not M.global_step_controller then return end

        local field_w = 16 -- Field width (assumed square)
        local ox = M.viewport_anchor.x + math.ceil(field_w / 16) * 16
        local oy = M.viewport_anchor.y
        local oz = M.viewport_anchor.z

        minetest.log("action", string.format(
            "[voxel_automata] Cadence render starting: anchor=(%d,%d,%d) size=%dx%dx%d",
            ox, oy, oz, field_w, field_w, field_w))

        -- Walk the field; for each voxel query its cadence, pick the node.
        local vm = VoxelManip()
        local world_min = { x = ox, y = oy, z = oz }
        local world_max = { x = ox + field_w - 1, y = oy + field_w - 1, z = oz + field_w - 1 }
        local emin, emax = vm:read_from_map(world_min, world_max)
        local data = vm:get_data()
        local area = VoxelArea:new({ MinEdge = emin, MaxEdge = emax })

        local cadence_ids = {}
        local cadence_id_set = {}
        for i = 1, 32 do
            local id = minetest.get_content_id(
                string.format("voxel_automata:cadence_%02d", i))
            cadence_ids[i] = id
            cadence_id_set[id] = true
        end
        local air_id = minetest.get_content_id("air")
        cadence_id_set[air_id] = true

        for z = 0, field_w - 1 do
            for y = 0, field_w - 1 do
                for x = 0, field_w - 1 do
                    local vi = area:indexp({ x = ox + x, y = oy + y, z = oz + z })
                    -- Skip voxels already occupied by something placed in the
                    -- field (seamplane, infinity contract, ...) so the overlay
                    -- doesn't stomp it back to a cadence-color block.
                    if cadence_id_set[data[vi]] then
                        local c = va.va_sc_cadence_lookup(M.global_step_controller, x, y, z)
                        local node_id = cadence_ids[math.min(c, 32)] or air_id
                        data[vi] = node_id
                    end
                end
            end
        end

        vm:set_data(data)
        vm:write_to_map()
        vm:update_map()
    end

    -- SeamPlane node definitions
    if minetest.get_modpath("mesecons") then
        minetest.register_node("voxel_automata:seam_off", {
            description = "SeamPlane (off)",
            tiles = {
                "voxel_automata_seam_off_para.png", -- top
                "voxel_automata_seam_off_para.png", -- bottom
                "voxel_automata_seam_off_perp.png", -- right (perpendicular)
                "voxel_automata_seam_off_perp.png", -- left (perpendicular)
                "voxel_automata_seam_off_para.png", -- front
                "voxel_automata_seam_off_para.png", -- back
            },
            paramtype = "light",
            paramtype2 = "facedir",
            is_ground_content = false,
            groups = { oddly_breakable_by_hand = 3 },
            mesecons = {
                effector = {
                    action_on = function(pos, node)
                        if not M.global_step_controller then return end
                        local meta = minetest.get_meta(pos)
                        local axis = meta:get_int("bisect_axis")
                        local fcx = meta:get_int("field_x")
                        local fcy = meta:get_int("field_y")
                        local fcz = meta:get_int("field_z")
                        local seam_coord = axis == 0 and fcx or (axis == 1 and fcy or fcz)

                        local ox = M.viewport_anchor.x + math.ceil(16 / 16) * 16
                        local oy = M.viewport_anchor.y
                        local oz = M.viewport_anchor.z
                        minetest.log("action", string.format(
                            "[voxel_automata] seam activated: world_pos=%s field=(%d,%d,%d) axis=%d seam_coord=%d anchor_origin=(%d,%d,%d)",
                            minetest.pos_to_string(pos), fcx, fcy, fcz, axis, seam_coord, ox, oy, oz))

                        local point = find_seam_point(axis, seam_coord, fcx, fcy, fcz)
                        if not point then
                            minetest.log("warning",
                                "[voxel_automata] SeamPlane at " .. minetest.pos_to_string(pos) ..
                                " does not intersect any cadence leaf, ignoring")
                            return
                        end

                        minetest.log("action", string.format(
                            "[voxel_automata] seam point field=(%d,%d,%d) world=(%d,%d,%d)",
                            point[1], point[2], point[3],
                            ox + point[1], oy + point[2], oz + point[3]))

                        local lo_cadence = compute_cadence_for_halfspace(axis, seam_coord, "lo")
                        local hi_cadence = compute_cadence_for_halfspace(axis, seam_coord, "hi")
                        va.va_sc_cadence_bisect(M.global_step_controller,
                            point[1], point[2], point[3], axis, seam_coord, lo_cadence, hi_cadence)
                        meta:set_int("seam_px", point[1])
                        meta:set_int("seam_py", point[2])
                        meta:set_int("seam_pz", point[3])
                        minetest.swap_node(pos, { name = "voxel_automata:seam_on", param2 = node.param2 })
                        M.render_cadence_zones()
                    end,
                    rules = mesecon.rules.alldirs,
                }
            },
            on_place = function(itemstack, placer, pointed_thing)
                local above = pointed_thing.above
                local under = pointed_thing.under
                -- Determine axis from which face was clicked (which coordinate changed)
                local axis = 2 -- default to Z
                local param2 = 0
                if above.x ~= under.x then
                    axis = 0 -- X face clicked: "perp" should be on X-facing sides
                    param2 = 0
                elseif above.y ~= under.y then
                    axis = 1 -- Y face clicked: "perp" should be on Y-facing sides
                    param2 = 16
                else
                    -- Z face clicked: "perp" should be on Z-facing sides
                    param2 = 3
                end
                -- set_node clears any existing node metadata at this position, so it
                -- must run before the meta:set_int calls below, not after.
                minetest.set_node(above, { name = "voxel_automata:seam_off", param2 = param2 })
                local meta = minetest.get_meta(above)
                local ox = M.viewport_anchor.x + math.ceil(16 / 16) * 16
                meta:set_int("field_x", above.x - ox)
                meta:set_int("field_y", above.y - M.viewport_anchor.y)
                meta:set_int("field_z", above.z - M.viewport_anchor.z)
                meta:set_int("bisect_axis", axis)
                return itemstack
            end,
        })

        minetest.register_node("voxel_automata:seam_on", {
            description = "SeamPlane (on)",
            tiles = {
                "voxel_automata_seam_on_para.png", -- top
                "voxel_automata_seam_on_para.png", -- bottom
                "voxel_automata_seam_on_perp.png", -- right (perpendicular)
                "voxel_automata_seam_on_perp.png", -- left (perpendicular)
                "voxel_automata_seam_on_para.png", -- front
                "voxel_automata_seam_on_para.png", -- back
            },
            paramtype = "light",
            paramtype2 = "facedir",
            is_ground_content = false,
            groups = { oddly_breakable_by_hand = 3, not_in_creative_inventory = 1 },
            mesecons = {
                effector = {
                    action_off = function(pos, node)
                        if not M.global_step_controller then return end
                        local meta = minetest.get_meta(pos)
                        local axis = meta:get_int("bisect_axis")
                        local px = meta:get_int("seam_px")
                        local py = meta:get_int("seam_py")
                        local pz = meta:get_int("seam_pz")
                        local null_x = px - (axis == 0 and 1 or 0)
                        local null_y = py - (axis == 1 and 1 or 0)
                        local null_z = pz - (axis == 2 and 1 or 0)
                        M.pending_merge = {
                            null_x = null_x,
                            null_y = null_y,
                            null_z = null_z,
                            alt_x = px,
                            alt_y = py,
                            alt_z = pz
                        }
                        minetest.swap_node(pos, { name = "voxel_automata:seam_off", param2 = node.param2 })
                        minetest.log("action", "[voxel_automata] Merge queued, polling until phase converges")
                    end,
                    rules = mesecon.rules.alldirs,
                }
            },
        })
    else
        minetest.register_node("voxel_automata:seam_off", {
            description = "SeamPlane (off) [mesecons not available]",
            tiles = {
                "voxel_automata_seam_off_para.png",
                "voxel_automata_seam_off_para.png",
                "voxel_automata_seam_off_perp.png",
                "voxel_automata_seam_off_perp.png",
                "voxel_automata_seam_off_para.png",
                "voxel_automata_seam_off_para.png",
            },
            groups = { oddly_breakable_by_hand = 3 },
        })
        minetest.register_node("voxel_automata:seam_on", {
            description = "SeamPlane (on) [mesecons not available]",
            tiles = {
                "voxel_automata_seam_on_para.png",
                "voxel_automata_seam_on_para.png",
                "voxel_automata_seam_on_perp.png",
                "voxel_automata_seam_on_perp.png",
                "voxel_automata_seam_on_para.png",
                "voxel_automata_seam_on_para.png",
            },
            groups = { oddly_breakable_by_hand = 3 },
        })
    end

    -- Infinity contract nodes (Phase 9c)
    -- These create Infinity Contracts with fixed conductivity values
    local infinity_values = {
        { percent = 5,  value = 214748365,  tile_base = "voxel_automata_infinity_05" },
        { percent = 25, value = 1073741823, tile_base = "voxel_automata_infinity_25" },
        { percent = 50, value = 2147483647, tile_base = "voxel_automata_infinity_50" },
        { percent = 75, value = 3221225471, tile_base = "voxel_automata_infinity_75" },
        { percent = 95, value = 4080218880, tile_base = "voxel_automata_infinity_95" },
    }

    if minetest.get_modpath("mesecons") then
        for _, inf in ipairs(infinity_values) do
            local percent = inf.percent
            local value = inf.value
            local tile_base = inf.tile_base

            minetest.register_node("voxel_automata:infinity_" .. percent .. "_off", {
                description = "Infinity Contract " .. percent .. "% (off)",
                tiles = { tile_base .. "_off.png" },
                paramtype = "light",
                is_ground_content = false,
                groups = { oddly_breakable_by_hand = 3 },
                mesecons = {
                    effector = {
                        action_on = function(pos, node)
                            if not M.global_step_controller then return end
                            local meta = minetest.get_meta(pos)
                            local fcx = meta:get_int("field_x")
                            local fcy = meta:get_int("field_y")
                            local fcz = meta:get_int("field_z")
                            minetest.log("action", string.format(
                                "[voxel_automata] Infinity %d%% activated: world_pos=%s field=(%d,%d,%d)",
                                percent, minetest.pos_to_string(pos), fcx, fcy, fcz))
                            local result = va.va_sc_infinity_create(M.global_step_controller, fcx, fcy, fcz, value)
                            if result == 0 then
                                minetest.swap_node(pos, { name = "voxel_automata:infinity_" .. percent .. "_on" })
                                minetest.log("action",
                                    "[voxel_automata] Infinity " ..
                                    percent .. "% contract created at (" .. fcx .. "," .. fcy .. "," .. fcz .. ")")
                            else
                                minetest.log("warning", string.format(
                                    "[voxel_automata] Failed to create Infinity %d%% contract at field=(%d,%d,%d) (field size 16x16x16)",
                                    percent, fcx, fcy, fcz))
                            end
                        end,
                        rules = mesecon.rules.alldirs,
                    }
                },
                on_place = function(itemstack, placer, pointed_thing)
                    local above = pointed_thing.above
                    minetest.set_node(above, { name = "voxel_automata:infinity_" .. percent .. "_off" })
                    local meta = minetest.get_meta(above)
                    -- Infinity contracts couple into the mass field (rendered by
                    -- render_field_grayscale), which is anchored at
                    -- viewport_anchor.x - 16, not the cadence overlay's anchor
                    -- (viewport_anchor.x + 16). Must match render_field_grayscale's
                    -- field_anchor exactly or field coordinates land out of bounds.
                    local ox = M.viewport_anchor.x - math.ceil(16 / 16) * 16
                    meta:set_int("field_x", above.x - ox)
                    meta:set_int("field_y", above.y - M.viewport_anchor.y)
                    meta:set_int("field_z", above.z - M.viewport_anchor.z)
                    return itemstack
                end,
            })

            minetest.register_node("voxel_automata:infinity_" .. percent .. "_on", {
                description = "Infinity Contract " .. percent .. "% (on)",
                tiles = { tile_base .. "_on.png" },
                paramtype = "light",
                is_ground_content = false,
                groups = { oddly_breakable_by_hand = 3, not_in_creative_inventory = 1 },
                mesecons = {
                    effector = {
                        action_off = function(pos, node)
                            if not M.global_step_controller then return end
                            local meta = minetest.get_meta(pos)
                            local fcx = meta:get_int("field_x")
                            local fcy = meta:get_int("field_y")
                            local fcz = meta:get_int("field_z")
                            local result = va.va_sc_infinity_destroy(M.global_step_controller, fcx, fcy, fcz)
                            if result == 0 then
                                minetest.swap_node(pos, { name = "voxel_automata:infinity_" .. percent .. "_off" })
                                minetest.log("action", "[voxel_automata] Infinity " .. percent .. "% contract destroyed")
                            else
                                minetest.log("warning",
                                    "[voxel_automata] Failed to destroy Infinity " .. percent .. "% contract")
                            end
                        end,
                        rules = mesecon.rules.alldirs,
                    }
                },
            })
        end
    else
        for _, inf in ipairs(infinity_values) do
            local percent = inf.percent
            local tile_base = inf.tile_base

            minetest.register_node("voxel_automata:infinity_" .. percent .. "_off", {
                description = "Infinity Contract " .. percent .. "% (off) [mesecons not available]",
                tiles = { tile_base .. "_off.png" },
                groups = { oddly_breakable_by_hand = 3 },
            })

            minetest.register_node("voxel_automata:infinity_" .. percent .. "_on", {
                description = "Infinity Contract " .. percent .. "% (on) [mesecons not available]",
                tiles = { tile_base .. "_on.png" },
                groups = { oddly_breakable_by_hand = 3 },
            })
        end
    end
end
