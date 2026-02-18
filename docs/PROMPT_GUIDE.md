# VCR Prompt Guide: The Golden Path

To get perfect renders on the first try, your prompt should follow this 4-step structure.

## 1. Frame the Target

Specify the aspect ratio and safety margin.

- **Good**: "Create a 9:16 vertical render for mobile."
- **Better**: "Create a 9:16 vertical render with a 15% safety gutter at the edges to prevent clipping."

## 2. Dictate Math Stability

Prevent "hallucinated" or flickering geometry by providing a mathematical strategy.

- **For Curves**: "Use a High-Density Capsule Chain (120+ segments) for geometric continuity."
- **For Solids**: "Use a robust, rotation-invariant SDF to prevent artifacts at 90-degree turns."

## 3. Detail the Aesthetic

VCR shines with dual-tone lighting and rim lights.

- **Lighting**: "Strong Purple primary light (Top-Left) + Teal rim light (Bottom-Right) to define volume."
- **Material**: "Matte black surface vs Sleek metallic highlights."

## 4. Mandate the "Truth Loop"

Force the AI to verify its own work before presenting it.

- **The Loop**: "Before delivery, you MUST generate a **3x3 Contact Sheet** using `scripts/vcr_contact_sheet.py` as proof that the framing is perfect throughout the entire 10-second animation."

---

### Example "One-Liner" for Agents
>
> *"Create a 9:16 vertical Moire Sphere with purple/teal lighting. Use a 20% safety margin and delivery MUST include a contact sheet verification from `vcr_contact_sheet.py`."*
