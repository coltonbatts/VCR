# VCR Onboarding (Quick Path)

## 1. Install

```bash
curl -fsSL https://raw.githubusercontent.com/coltonbatts/VCR/main/scripts/install.sh | bash
vcr --version
```

## 2. Start from a Template (Path-Safe)

Run from the repository root and keep the copied file under `examples/` so relative shader paths keep working.

```bash
ls examples/*.vcr
cp examples/ai_company_hero.vcr examples/my_brand.vcr
```

## 3. Render

```bash
vcr render ./examples/my_brand.vcr -o ./renders/my_brand.mov
```

## 4. Preview While Editing

- Works in standard builds: `vcr watch ./examples/my_brand.vcr`
- Optional preview window (`play` feature required): `cargo run --release --features play -- play ./examples/my_brand.vcr`

## 5. Output Profiles and Alpha

- `vcr render` forces ProRes 4444 for `.mov` output (alpha-safe default contract).
- `vcr build` follows `environment.encoding.prores_profile` in the manifest.
- For alpha with `vcr build`, set:

```yaml
environment:
  encoding:
    prores_profile: prores4444
```
