#!/usr/bin/env python3
from PIL import Image
import json

# Load the cadence palette image
img = Image.open('mod/textures/voxel_automata_cadence_palette.png')
img = img.convert('RGB')

# Extract RGB values
pixels = list(img.getdata())
width, height = img.size

print(f"Image size: {width}x{height}")
print(f"Total pixels: {len(pixels)}")

# Build a list of unique RGB values in order
palette = []
for r, g, b in pixels:
    palette.append({'r': r, 'g': g, 'b': b})

print(f"\nPalette ({len(palette)} colors):")
for i, color in enumerate(palette):
    print(f"{i:3d}: #{color['r']:02x}{color['g']:02x}{color['b']:02x}")

# Save as JSON for later use
with open('palette.json', 'w') as f:
    json.dump(palette, f, indent=2)

print("\nSaved to palette.json")
