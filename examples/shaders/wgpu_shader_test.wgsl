fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
  let center = vec2<f32>(0.5, 0.5);
  let p = uv - center;
  let radius = length(p);
  let wave = 0.5 + 0.5 * sin(time * 2.0 + radius * 18.0);
  let vignette = smoothstep(0.9, 0.2, radius);
  let tint = vec3<f32>(uv.x, uv.y, 1.0 - uv.x);
  let color = tint * wave * vignette;
  let alpha = clamp(vignette, 0.0, 1.0);
  let _res = resolution;
  return vec4<f32>(color, alpha);
}
