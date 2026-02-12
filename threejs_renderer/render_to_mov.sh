#!/bin/bash
set -e

# Configuration
WIDTH=1920
HEIGHT=1080
FPS=30
DURATION=8
OUTPUT="$1"

if [ -z "$OUTPUT" ]; then
    OUTPUT="../renders/threejs_pyramid.mov"
fi

echo "Rendering ThreeJS pyramid to $OUTPUT"
echo "Resolution: ${WIDTH}x${HEIGHT} @ ${FPS}fps"
echo "Duration: ${DURATION}s"

# Create temp directory for frames
TEMP_DIR=$(mktemp -d)
echo "Temp directory: $TEMP_DIR"

# Render all frames as PNG
echo "Rendering frames..."
node render_pyramid.js \
    --width $WIDTH \
    --height $HEIGHT \
    --fps $FPS \
    --duration $DURATION \
    --output "$TEMP_DIR/frame_" \
    2>&1 | grep -v "^$"

# Encode to ProRes 4444 with FFmpeg
echo "Encoding to ProRes 4444..."
ffmpeg -y \
    -framerate $FPS \
    -i "$TEMP_DIR/frame_%04d.png" \
    -c:v prores_ks \
    -profile:v 4444 \
    -pix_fmt yuva444p10le \
    -vendor ap10 \
    "$OUTPUT" \
    2>&1 | grep -E "(frame=|Duration:|Output|Stream|Video:)"

# Cleanup
rm -rf "$TEMP_DIR"

echo "Done! Wrote $OUTPUT"
