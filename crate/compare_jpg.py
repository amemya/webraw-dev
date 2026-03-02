from PIL import Image

def find_highlight(img_path):
    try:
        img = Image.open(img_path)
        img = img.convert('RGB')
        w, h = img.size
        
        # We look for a line where the pixels transition from low to very high (clipping)
        # We will scan the middle area of the image for MaxC > 250
        
        for y in range(h // 4, h * 3 // 4, 10):
            for x in range(w // 4, w * 3 // 4):
                r, g, b = img.getpixel((x, y))
                if max(r, g, b) > 250:
                    # Found a clip point! Let's print the neighborhood
                    print(f"\n--- Found clip at x={x}, y={y} in {img_path} ---")
                    for cx in range(x - 20, x + 5):
                        pr, pg, pb = img.getpixel((cx, y))
                        print(f"{cx:4d} | {pr:3d} | {pg:3d} | {pb:3d} | {max(pr,pg,pb):3d}")
                    return
        print(f"No clipped highlight found in {img_path}")
    except Exception as e:
        print(f"Error: {e}")

if __name__ == '__main__':
    find_highlight('out.ppm')
    find_highlight('../sample_jpegs/_89R3389.jpg')
