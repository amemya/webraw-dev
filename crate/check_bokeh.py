import sys
import numpy as np

# Read PPM assuming P6
filename = sys.argv[1] if len(sys.argv) > 1 else "out_no_matrix.ppm"
with open(filename, "rb") as f:
    header = f.readline().decode('ascii')
    assert header.strip() == "P6"
    
    dims = f.readline().decode('ascii')
    while dims.startswith('#'):
        dims = f.readline().decode('ascii')
    w, h = map(int, dims.split())
    
    maxval = f.readline().decode('ascii')
    while maxval.startswith('#'):
        maxval = f.readline().decode('ascii')
    
    data = f.read()

img = np.frombuffer(data, dtype=np.uint8).reshape((h, w, 3))
img = img.astype(np.float32) / 255.0

# Extract a small square covering the bokeh
cy, cx = h//2, w//2
crop = img[cy-400:cy+400, cx-400:cx+400]

# Let's find the brightest pixel in the crop
max_idx = np.unravel_index(np.argmax(np.mean(crop, axis=2)), crop.shape[:2])
by, bx = max_idx

# Extract a 1D slice across the bokeh (horizontal line through the center)
slice_x = np.arange(bx - 100, bx + 100)
slice_y = np.full_like(slice_x, by)

# Ensure within bounds
valid = (slice_x >= 0) & (slice_x < crop.shape[1])
slice_x = slice_x[valid]
slice_y = slice_y[valid]

pixels = crop[slice_y, slice_x]

print("Pixel slice across highlight:")
print("Idx | R      | G      | B      | MaxC")
print("----|--------|--------|--------|-------")
for i, (r, g, b) in enumerate(pixels):
    if i < 70 or i > 110: continue
    max_c = max(r, g, b)
    print(f"{i:3d} | {r:6.4f} | {g:6.4f} | {b:6.4f} | {max_c:6.4f}")

