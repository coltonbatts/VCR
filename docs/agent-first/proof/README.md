# First production proof: reusable lower-third system

Scene: [`examples/agent/lower_third_v1.vcr`](../../../examples/agent/lower_third_v1.vcr), one
versioned, parameterized alpha lower third (1920×1080, 60 fps, 5 s, ProRes 4444).

**Creative status: fixture.** The copy ("SAMPLE NAME" / "SAMPLE TITLE"), palette and layout are
placeholders chosen for engine proof. They are *not* approved brand identity. A human must approve
or replace them before production use. The brief left text, palette and assets unspecified and the
prompt gate reported them so (`creative_inputs: unspecified`); nothing was silently defaulted as
brand.

## Workflow that produced it (all commands real, engine `vcr.agent/1`)

1. `vcr prompt --json --in lower_third_request.yaml -o lower_third_request.normalized.yaml` →
   `status: ok`, no unknowns; only `output.fps` (=render fps) was a specification default.
   *(No packs referenced, so no pack contact sheet was required.)*
2. Authored the manifest from `normalized_spec`.
3. `check` / `lint` / `explain` (preflight: ready, software) → ok.
4. `inspect` found a real authoring defect on the first draft: `panel` and `accent_stripe` were
   *visible by state but drew nothing* (`layer.renders_nothing`), because procedural geometry is
   normalized 0–1 and I had written pixels. `check` and `lint` had both passed. Fixed in the
   manifest; re-inspected: exact hold bounds panel x 96–935 / y 820–985, text inside, no edge contact.
5. `render --backend software --json` (atomic publish, provenance) → 300 frames in ~34 s (release).
6. `verify --manifest … --expect-transparency required` → all checks pass; transparency measured on
   300 decoded frames (min alpha 0).
7. Re-render at a different filename: raster, decoded and encoded hashes identical; metadata
   sidecar byte-identical.

Negative checks performed on derivatives of the delivered file (all detected, exit 3): wrong
duration (trimmed), wrong codec (H.264), alpha-capable pix_fmt but opaque pixels, no-alpha pix_fmt,
stale (verified against changed params), modified byte, truncated file.

## What this does and does not establish

- ✅ One correct instance end to end; the verification chain detects the failure modes above.
- ✅ The twelve-variant matrix ([`variant_matrix.yaml`](variant_matrix.yaml)) is defined; see the
  handoff for whether the scripted run of it passed.
- ❌ Not established: human approval of the creative content, recovery behavior under fault
  injection with real agents, any autonomous-agent success rate, GPU-backend output, cross-machine
  encoded-byte identity.

## Evidence (small files committed under [`evidence/`](evidence/))

Contact sheet, inspection document, provenance and metadata for the baseline render, the verify
result, the variant report, and the contract-check report. The 24 MB `.mov` itself is not committed;
regenerate it with:

```bash
cargo build --release
target/release/vcr --backend software render examples/agent/lower_third_v1.vcr \
    -o renders/lower_third/lt_v1_baseline.mov --json
target/release/vcr verify renders/lower_third/lt_v1_baseline.mov \
    --manifest examples/agent/lower_third_v1.vcr --expect-transparency required --json
```
