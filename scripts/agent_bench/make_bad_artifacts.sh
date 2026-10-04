#!/usr/bin/env bash
# Build the T10 artifact set: seven exports of one transparent scene, one good, six wrong in
# different ways. Writes OUT/artifacts/{a..g}.mov (+ provenance where the engine made the file) and
# OUT/artifacts_truth.json (scorer-only; never show it to the agent under test).
#
# usage: scripts/agent_bench/make_bad_artifacts.sh OUT_DIR [path/to/vcr]
set -euo pipefail
OUT="${1:?usage: make_bad_artifacts.sh OUT_DIR [vcr]}"
VCR="${2:-target/release/vcr}"
VCR="$(cd "$(dirname "$VCR")" && pwd)/$(basename "$VCR")"
mkdir -p "$OUT/artifacts"
cd "$OUT/artifacts"
rm -f ./* 2>/dev/null || true

cat > scene.vcr <<'YAML'
version: 1
environment:
  resolution: { width: 1280, height: 720 }
  fps: 30
  duration: 3.0
  encoding: { prores_profile: prores4444 }
layers:
  - id: badge
    procedural:
      kind: circle
      center: { x: 0.5, y: 0.5 }
      radius: 0.2
      color: { r: 0.9, g: 0.3, b: 0.2, a: 1.0 }
YAML

render() { "$VCR" --quiet --backend software render "$1" -o "$2" --json >/dev/null 2>&1; }
SRC=_good.mov
render scene.vcr "$SRC"

# ok
cp "$SRC" a.mov; cp "$SRC.provenance.json" a.mov.provenance.json
# wrong_duration: first 2 s only, stream-copied (no provenance: not engine output)
ffmpeg -v error -y -i "$SRC" -t 2 -c copy b.mov
# wrong_codec
ffmpeg -v error -y -i "$SRC" -c:v libx264 -pix_fmt yuv420p c.mov
# no_transparency: alpha-capable pix_fmt, every pixel opaque
ffmpeg -v error -y -i "$SRC" -vf "format=rgba,lutrgb=a=255" -c:v prores_ks -profile:v 4 -pix_fmt yuva444p10le d.mov
# stale: engine output of a DIFFERENT scene, provenance copied from it, presented as this scene's export
sed 's/radius: 0.2/radius: 0.3/' scene.vcr > _other.vcr
render _other.vcr e.mov
# modified: engine output with provenance, then a byte flipped
cp "$SRC" f.mov; cp "$SRC.provenance.json" f.mov.provenance.json
python3 - <<'PY'
b = bytearray(open("f.mov", "rb").read()); b[len(b)//2] ^= 0xFF; open("f.mov", "wb").write(b)
PY
# unreadable: truncated
head -c "$(( $(wc -c < "$SRC") / 3 ))" "$SRC" > g.mov

# provenance sidecars would give the stale/modified answers away by filename; that is intended:
# the agent is expected to use `vcr verify`, which is what reads them.
rm -f _other.vcr _other.vcr.* "$SRC" "$SRC.metadata.json" "$SRC.provenance.json" e.mov.metadata.json ./*.metadata.json
rmdir renders 2>/dev/null || true
cd ..
cat > artifacts_truth.json <<'JSON'
{
  "a.mov": {"acceptable": true,  "problem": "ok"},
  "b.mov": {"acceptable": false, "problem": "wrong_duration"},
  "c.mov": {"acceptable": false, "problem": "wrong_codec"},
  "d.mov": {"acceptable": false, "problem": "no_transparency"},
  "e.mov": {"acceptable": false, "problem": "stale"},
  "f.mov": {"acceptable": false, "problem": "modified"},
  "g.mov": {"acceptable": false, "problem": "unreadable"}
}
JSON
echo "wrote $OUT/artifacts (7 files) and $OUT/artifacts_truth.json"
