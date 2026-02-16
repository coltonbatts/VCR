# Welcome to VCR: The Golden Path

VCR (Video Component Renderer) is motion graphics for people who think code is faster than Adobe. This guide will get you from "Just Installed" to "Cool Animation" in two minutes.

## The VCR Philosophy

1. **Motion is Data**: Everything is a YAML file.
2. **Deterministic**: Same file, same pixels, every time.
3. **Agent-First**: You don't always have to write the YAML; you can just talk to VCR.

---

## 🚀 The Two-Minute Takt

### 1. Install (The One-Liner)

If you have Rust and FFmpeg, run this:

```bash
curl -fsSL https://raw.githubusercontent.com/coltonbatts/VCR/main/scripts/install.sh | bash
```

### 2. Pick your Vibe (Use a Starter Kit)

Don't start from a blank page. Copy one of our pro templates:

```bash
# Look at the examples
ls examples/*.vcr

# Copy the AI Company Hero template
cp examples/ai_company_hero.vcr my_brand.vcr
```

### 3. Customize with the Agent

Don't worry about the YAML syntax yet. Ask the VCR AI agent to change it for you:
> "Hey, change the company name in `my_brand.vcr` to 'CYBER-DYNE' and make the sphere rotate faster."

### 4. Render

```bash
vcr render my_brand.vcr -o my_render.mov
```

---

## 🤖 Talking to the VCR Agent

The best way to use VCR is as a **Pair Programmer**. Instead of clicking buttons in a GUI, you describe your intent.

### Bad Request
>
> "Make a video." (Too vague)

### 🏆 Pro Request
>
> "I need a 5-second, 60fps intro for my AI company. Use the `neural_sphere.wgsl` shader. The background should be a dark blue-to-black gradient. Add 'VCR ENGINE' in large Geist Pixel text that fades in after 1 second."

---

## 🛠 Pro Tips

- **Live Preview**: Use `vcr play my_brand.vcr` to see changes as you save.
- **Alpha is Free**: VCR renders ProRes 4444 by default if you have a transparent background. Perfect for dropping into Premiere or DaVinci.
- **Expression Power**: Use `pos_x: "960 + sin(t * 0.1) * 200"` to make things move without keyframes.

**Welcome to the terminal. Let's make something cool.**
