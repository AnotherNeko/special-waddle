# Development Notes

## Luanti Flatpak Paths

- **Luanti root**: `~/.var/app/org.luanti.luanti/.minetest/`
- **Debug log**: `~/.var/app/org.luanti.luanti/.minetest/debug.txt`
- **Mods directory**: `~/.var/app/org.luanti.luanti/.minetest/mods/`
- **Worlds directory**: `~/.var/app/org.luanti.luanti/.minetest/worlds/`

Use these paths when debugging Luanti issues. The flatpak's app data is at `~/.var/app/org.luanti.luanti/`.

## Known Issues & Fixes

### Phase 4: Region Extraction Returns 0 Bytes

When calling `/ca_render`, the extraction was returning 0 bytes because the automaton grid (0-15 range) 
was being queried at player coordinates which are typically in large negative or positive ranges far outside the grid.

**Fix**: The render_region function should map player coordinates to the automaton's coordinate space (0-15 range)
rather than trying to extract from the world coordinates directly.
