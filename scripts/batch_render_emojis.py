import os
import json
import subprocess

def run_cmd(cmd, cwd=None):
    print(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    if result.returncode != 0:
        print(f"Error: {result.stderr}")
    else:
        print(result.stdout)
    return result

source_dir = "/System/Volumes/Data/Users/coltonbatts/Projects/assets/creative-library/Plugins-Templates/Retro Emoji Motion Pack/Main"
# Output relative to VCR root
output_rel_dir = "out/batch_emojis"
manifest_rel_dir = "manifests/render/batch"
vcr_root = "/Users/coltonbatts/Desktop/VCR"
vcr_path = "./target/debug/vcr"

os.makedirs(os.path.join(vcr_root, output_rel_dir), exist_ok=True)
os.makedirs(os.path.join(vcr_root, manifest_rel_dir), exist_ok=True)

# 1. Get existing library items
lib_list = run_cmd([vcr_path, "library", "list"], cwd=vcr_root).stdout
existing_ids = [line.split()[0] for line in lib_list.splitlines() if line.strip()]

for i in range(1, 33):
    item_id = f"retro-emoji-{i:02d}"
    filename = f"Retro Emoji {i:02d}.mov"
    source_path = os.path.join(source_dir, filename)
    
    if not os.path.exists(source_path):
        print(f"Skipping {item_id}, source not found: {source_path}")
        continue

    # 2. Register to library if missing
    if item_id not in existing_ids:
        print(f"Registering {item_id}...")
        run_cmd([vcr_path, "library", "add", source_path, "--id", item_id, "--type", "video"], cwd=vcr_root)

    # 3. Create manifest
    manifest_abs_path = os.path.join(vcr_root, manifest_rel_dir, f"{item_id}.vcr")
    manifest_content = f"""version: 1
environment:
  resolution: {{ width: 1920, height: 1080 }}
  fps: 30
  duration: 10

layers:
  - id: {item_id}
    z_index: 10
    position: [960, 540]
    anchor: center
    scale: [0.75, 0.75]
    video:
      path: "library:{item_id}"
"""
    with open(manifest_abs_path, 'w') as f:
        f.write(manifest_content)

    # 4. Render
    # Use relative path for output to satisfy security check
    rel_output_mov = os.path.join(output_rel_dir, f"{item_id}.mov")
    rel_manifest_path = os.path.join(manifest_rel_dir, f"{item_id}.vcr")
    
    print(f"Rendering {item_id} to {rel_output_mov}...")
    run_cmd([vcr_path, "render", rel_manifest_path, "-o", rel_output_mov], cwd=vcr_root)

print("Batch processing complete.")
