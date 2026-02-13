# Rapier + Three.js Handshake Boilerplate

A hardened, deterministic boilerplate for integrating **Rapier3D** (WASM) with **Three.js** in a Vite + TypeScript environment. Optimized for VCR (Video Component Renderer) workflows including alpha transparency and fixed-timestep physics.

## 🚀 Features

- **Deterministic Physics**: Implements a fixed $1/60$s timestep accumulator for bit-exact simulation results.
- **VCR Ready**: Pre-configured for "print on alpha" workflows with transparent background support.
- **Premium Aesthetics**: High-contrast "Retrowave" style with metallic surfaces and neon lighting.
- **Modular Structure**: Clean TypeScript Class-based implementation.
- **WASM Optimized**: Uses `@dimforge/rapier3d-compat` for seamless bundler integration.

## 🛠️ Quickstart

### Installation

```bash
npm install
```

### Development

```bash
npm run dev
```

Open `http://localhost:5175/` (default).

### VCR Alpha Capture

To capture the simulation for layering in VCR:

```bash
vcr ascii capture --source tool:chafa --url http://localhost:5175/ --bg-alpha 0.0 --out rapier_overlay.mov
```

## 📜 Project Structure

- `src/main.ts`: Core application logic (Physics + Graphics sync).
- `index.html`: Entry point with transparent overlay settings.
- `tsconfig.json`: TypeScript configuration.
- `vite.config.ts`: Vite development server configuration.

## ⚖️ License

MIT
