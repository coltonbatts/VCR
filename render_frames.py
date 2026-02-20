import json
import os
from PIL import Image, ImageDraw, ImageFont

def render_ascii_frames(json_path, font_path, out_dir):
    with open(json_path, 'r', encoding='utf-8') as f:
        frames = json.load(f)
    
    if not os.path.exists(out_dir):
        os.makedirs(out_dir)
    
    # Grid: 100 cols x 54 rows
    # Cell size: 10x20
    cell_w = 10
    cell_h = 20
    img_w = 100 * cell_w
    img_h = 54 * cell_h
    
    # Load font
    try:
        font = ImageFont.truetype(font_path, 18) # 18pt should fit well in 10x20
    except Exception as e:
        print(f"Failed to load font: {e}")
        return

    for i, frame in enumerate(frames):
        # Create image with transparent background (RGBA)
        img = Image.new('RGBA', (img_w, img_h), (0, 0, 0, 0))
        draw = ImageDraw.Draw(img)
        
        lines = frame.split('\n')
        for row_idx, line in enumerate(lines):
            for col_idx, char in enumerate(line):
                if char.isspace():
                    continue
                # Draw white character
                draw.text((col_idx * cell_w, row_idx * cell_h), char, font=font, fill=(255, 255, 255, 255))
        
        out_path = os.path.join(out_dir, f"frame_{i:04d}.png")
        img.save(out_path)
        if i % 20 == 0:
            print(f"Rendered {i}/{len(frames)} frames...")

    print(f"Finished rendering {len(frames)} frames to {out_dir}")

if __name__ == "__main__":
    render_ascii_frames('jcvd_frames.json', 'assets/fonts/geist_pixel/GeistPixel-Line.ttf', 'jcvd_frames')
