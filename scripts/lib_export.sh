#!/bin/bash

# VCR Element Library Export Script
# Usage: ./scripts/lib_export.sh <manifest_path> [mov_path]

set -e

MANIFEST=$1
MOV_PATH=$2

if [ -z "$MANIFEST" ]; then
    echo "Usage: $0 <manifest_path> [mov_path]"
    exit 1
fi

if [ ! -f "$MANIFEST" ]; then
    echo "Error: Manifest $MANIFEST not found."
    exit 1
fi

# Get element ID from manifest basename
ELEMENT_ID=$(basename "$MANIFEST" .vcr)
LIB_ROOT="library/elements"

# If MOV_PATH is not provided, try to find it in renders/
if [ -z "$MOV_PATH" ]; then
    # Look for common patterns
    POSSIBLE_MOV="renders/$ELEMENT_ID.mov"
    if [ -f "$POSSIBLE_MOV" ]; then
        MOV_PATH="$POSSIBLE_MOV"
    else
        echo "Error: Could not find .mov for $ELEMENT_ID in renders/. Please specify mov_path."
        exit 1
    fi
fi

if [ ! -f "$MOV_PATH" ]; then
    echo "Error: MOV file $MOV_PATH not found."
    exit 1
fi

echo "Exporting $ELEMENT_ID to $LIB_ROOT..."

# Copy files
cp "$MANIFEST" "$LIB_ROOT/$ELEMENT_ID.vcr"
cp "$MOV_PATH" "$LIB_ROOT/$ELEMENT_ID.mov"

# Check for preview images
PREVIEW_PNG="renders/${ELEMENT_ID}_preview.png"
if [ -f "$PREVIEW_PNG" ]; then
    cp "$PREVIEW_PNG" "$LIB_ROOT/${ELEMENT_ID}_preview.png"
fi

echo "Success! Element '$ELEMENT_ID' is now in $LIB_ROOT"
echo "You can now find the movie at $LIB_ROOT/$ELEMENT_ID.mov"
