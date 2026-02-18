import sys
import os
import subprocess
from PIL import Image

def generate_contact_sheet(video_path, output_path):
    if not os.path.exists(video_path):
        print(f"Error: {video_path} not found")
        return

    # 1. Get total frames from ffprobe
    cmd = [
        "ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0",
        "-show_entries", "stream=nb_read_frames", "-of", "default=nokey=1:noprint_wrappers=1",
        video_path
    ]
    try:
        total_frames = int(subprocess.check_output(cmd).decode().strip())
    except:
        total_frames = 300 # Default fallback for 10s @ 30fps

    # 2. Select 9 frames evenly spaced
    frame_indices = [int(i * (total_frames - 1) / 8) for i in range(9)]
    temp_dir = "renders/temp_frames"
    os.makedirs(temp_dir, exist_ok=True)
    
    frames = []
    for i, idx in enumerate(frame_indices):
        frame_file = os.path.join(temp_dir, f"frame_{i}.png")
        cmd = [
            "ffmpeg", "-y", "-i", video_path,
            "-vf", f"select=eq(n\,{idx})", "-vframes", "1",
            frame_file
        ]
        subprocess.run(cmd, capture_output=True)
        if os.path.exists(frame_file):
            frames.append(Image.open(frame_file))

    if not frames:
        print("Error: No frames extracted")
        return

    # 3. Create Mosaic (3x3)
    w, h = frames[0].size
    contact_sheet = Image.new("RGBA", (w * 3, h * 3), (0, 0, 0, 0))
    
    for i, frame in enumerate(frames):
        x = (i % 3) * w
        y = (i // 3) * h
        contact_sheet.paste(frame, (x, y))

    # 4. Save and Clean up
    contact_sheet.save(output_path)
    print(f"Contact sheet saved to {output_path}")
    
    for i in range(len(frames)):
        os.remove(os.path.join(temp_dir, f"frame_{i}.png"))
    os.rmdir(temp_dir)

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python vcr_contact_sheet.py <video_path> <output_path>")
    else:
        generate_contact_sheet(sys.argv[1], sys.argv[2])
