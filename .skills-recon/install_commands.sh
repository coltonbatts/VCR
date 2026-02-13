#!/bin/sh
# Candidate installs from skills.sh for VCR research and adaptation.
# Run these manually as needed in your preferred skill toolchain.

set -u

echo "skills.sh candidate pages:"
echo "https://skills.sh/remotion-dev/skills/remotion-best-practices"
echo "https://skills.sh/digitalsamba/claude-code-video-toolkit/ffmpeg"
echo "https://skills.sh/mrgoonie/claudekit-skills/media-processing"
echo "https://skills.sh/openai/skills/figma"
echo "https://skills.sh/apollographql/skills/rust-best-practices"
echo "https://skills.sh/leonardomso/rust-skills/rust-skills"

echo ""
echo "Suggested local workflow:"
echo "1) Inspect each page and pull examples into .skills/* modules."
echo "2) Keep adaptations engine-agnostic and token-driven."
echo "3) Validate with: vcr check / vcr render-frame / vcr build."
