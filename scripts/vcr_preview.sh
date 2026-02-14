#!/bin/bash
# vcr_preview.sh - Quickly render a few frames of a VCR manifest for verification.

MANIFEST=$1
OUTPUT=${2:-"preview.mov"}
FRAMES=${3:-"30"} # Default to 1 second at 30fps

if [ -z "$MANIFEST" ]; then
    echo "Usage: ./vcr_preview.sh <manifest.vcr> [output.mov] [frames]"
    exit 1
fi

echo "Rendering frames $FRAMES of $MANIFEST to $OUTPUT..."

# Use the release binary
./target/release/vcr build "$MANIFEST" -o "$OUTPUT" --frames "$FRAMES"

if [ $? -eq 0 ]; then
    echo "Done! Output saved to $OUTPUT"
    # Logic to show metadata if it exists
    if [ -f "$OUTPUT.metadata.json" ]; then
        echo "Metadata generated at $OUTPUT.metadata.json"
    fi
else
    echo "Rendering failed."
    exit 1
fi
