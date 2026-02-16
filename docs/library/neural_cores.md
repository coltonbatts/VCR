# VCR Library: Neural Cores

Neural Cores are high-end, raymarching-based volumetric elements designed to showcase the power of the VCR shader engine. They provide a "hero" look suitable for title cards, background elements, or technical demos.

## The Aesthetic

- **Crystalline Structure**: ACHIEVED via recursive fractal folding or complex noise displacement.
- **Volumetric Lighting**: Emissive glow centered on the object that creates a "flare" effect (Solar Flare).
- **Spectral Glitch**: Subtle per-channel displacement that makes the element feel "alive" and technical.

## Implementation Guide

### 1. Raymarching Base (The Void)

All Neural Cores use a raymarching loop in the `shade()` function. This allows for complex 3D-like geometry without a traditional mesh.

```wgsl
fn map(p: vec3<f32>, u: ShaderUniforms) -> f32 {
    let p_rot = rotation_matrix(vec3<f32>(0.2, 1.0, 0.3), u.time * 0.05) * p;
    var d = length(p_rot) - 1.0; // Base Sphere
    
    // Neural Net Displacement
    let noise = sin(p_rot.x * 8.0 + u.time * 0.1) * sin(p_rot.y * 10.0) * sin(p_rot.z * 12.0) * 0.1;
    return d + noise;
}
```

### 2. Volumetric Glow

To create the characteristic "flare," we gather glow along the ray path using an exponential decay function.

```wgsl
for (var i = 0; i < 48; i++) {
    let pos = ro + rd * t;
    d = map(pos, u);
    glow += exp(-d * 6.0) * 0.02; // Volumetric gathering
    if (d < 0.001 || t > 10.0) { break; }
    t += d;
}
```

### 3. Alpha-First Rendering

For clean compositing, Neural Cores should be rendered on a transparent background.

- Set `void_background` alpha to `0.0`.
- Clamp the final color to its length to ensure the alpha channel matches the emissive intensity.

## Reference Shaders

- [Beyond the Ceiling](file:///Users/coltonbatts/Desktop/VCR/examples/shaders/beyond_the_ceiling.wgsl): The current state-of-the-art Neural Core.
- [Neural Sphere](file:///Users/coltonbatts/Desktop/VCR/examples/shaders/neural_sphere.wgsl): A node-and-connector variation.
