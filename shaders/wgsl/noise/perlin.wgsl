// Classic Perlin Noise for WGSL
// Based on Ken Perlin's improved noise (2002)
// Adapted for WGSL by VCR Standard Library

// Permutation table (same as simplex)
const PERM: array<u32, 256> = array<u32, 256>(
    151u, 160u, 137u, 91u, 90u, 15u, 131u, 13u, 201u, 95u, 96u, 53u, 194u, 233u, 7u, 225u,
    140u, 36u, 103u, 30u, 69u, 142u, 8u, 99u, 37u, 240u, 21u, 10u, 23u, 190u, 6u, 148u,
    247u, 120u, 234u, 75u, 0u, 26u, 197u, 62u, 94u, 252u, 219u, 203u, 117u, 35u, 11u, 32u,
    57u, 177u, 33u, 88u, 237u, 149u, 56u, 87u, 174u, 20u, 125u, 136u, 171u, 168u, 68u, 175u,
    74u, 165u, 71u, 134u, 139u, 48u, 27u, 166u, 77u, 146u, 158u, 231u, 83u, 111u, 229u, 122u,
    60u, 211u, 133u, 230u, 220u, 105u, 92u, 41u, 55u, 46u, 245u, 40u, 244u, 102u, 143u, 54u,
    65u, 25u, 63u, 161u, 1u, 216u, 80u, 73u, 209u, 76u, 132u, 187u, 208u, 89u, 18u, 169u,
    200u, 196u, 135u, 130u, 116u, 188u, 159u, 86u, 164u, 100u, 109u, 198u, 173u, 186u, 3u, 64u,
    52u, 217u, 226u, 250u, 124u, 123u, 5u, 202u, 38u, 147u, 118u, 126u, 255u, 82u, 85u, 212u,
    207u, 206u, 59u, 227u, 47u, 16u, 58u, 17u, 182u, 189u, 28u, 42u, 223u, 183u, 170u, 213u,
    119u, 248u, 152u, 2u, 44u, 154u, 163u, 70u, 221u, 153u, 101u, 155u, 167u, 43u, 172u, 9u,
    129u, 22u, 39u, 253u, 19u, 98u, 108u, 110u, 79u, 113u, 224u, 232u, 178u, 185u, 112u, 104u,
    218u, 246u, 97u, 228u, 251u, 34u, 242u, 193u, 238u, 210u, 144u, 12u, 191u, 179u, 162u, 241u,
    81u, 51u, 145u, 235u, 249u, 14u, 239u, 107u, 49u, 192u, 214u, 31u, 181u, 199u, 106u, 157u,
    184u, 84u, 204u, 176u, 115u, 121u, 50u, 45u, 127u, 4u, 150u, 254u, 138u, 236u, 205u, 93u,
    222u, 114u, 67u, 29u, 24u, 72u, 243u, 141u, 128u, 195u, 78u, 66u, 215u, 61u, 156u, 180u
);

fn perm(x: u32) -> u32 {
    return PERM[x & 255u];
}

// Fade function for smooth interpolation
fn fade(t: f32) -> f32 {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// Linear interpolation
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    return a + t * (b - a);
}

// Gradient function for 2D
fn grad2(hash: u32, x: f32, y: f32) -> f32 {
    let h = hash & 3u;
    let u = select(-x, x, (h & 1u) == 0u);
    let v = select(-y, y, (h & 2u) == 0u);
    return u + v;
}

// Gradient function for 3D
fn grad3(hash: u32, x: f32, y: f32, z: f32) -> f32 {
    let h = hash & 15u;
    let u = select(y, x, h < 8u);
    let v = select(select(x, z, h == 12u || h == 14u), y, h < 4u);
    return select(u, -u, (h & 1u) == 0u) + select(v, -v, (h & 2u) == 0u);
}

// 2D Perlin Noise
fn perlin_noise_2d(p: vec2<f32>) -> f32 {
    // Find unit grid cell containing point
    let pi = floor(p);
    let pf = fract(p);
    
    let ix = u32(pi.x) & 255u;
    let iy = u32(pi.y) & 255u;
    
    // Compute fade curves
    let u = fade(pf.x);
    let v = fade(pf.y);
    
    // Hash coordinates of the 4 corners
    let aa = perm(ix + perm(iy));
    let ab = perm(ix + perm(iy + 1u));
    let ba = perm(ix + 1u + perm(iy));
    let bb = perm(ix + 1u + perm(iy + 1u));
    
    // Blend results from the 4 corners
    let x1 = lerp(grad2(aa, pf.x, pf.y), grad2(ba, pf.x - 1.0, pf.y), u);
    let x2 = lerp(grad2(ab, pf.x, pf.y - 1.0), grad2(bb, pf.x - 1.0, pf.y - 1.0), u);
    
    return lerp(x1, x2, v);
}

// 3D Perlin Noise
fn perlin_noise_3d(p: vec3<f32>) -> f32 {
    // Find unit grid cell containing point
    let pi = floor(p);
    let pf = fract(p);
    
    let ix = u32(pi.x) & 255u;
    let iy = u32(pi.y) & 255u;
    let iz = u32(pi.z) & 255u;
    
    // Compute fade curves
    let u = fade(pf.x);
    let v = fade(pf.y);
    let w = fade(pf.z);
    
    // Hash coordinates of the 8 cube corners
    let aaa = perm(ix + perm(iy + perm(iz)));
    let aba = perm(ix + perm(iy + 1u + perm(iz)));
    let aab = perm(ix + perm(iy + perm(iz + 1u)));
    let abb = perm(ix + perm(iy + 1u + perm(iz + 1u)));
    let baa = perm(ix + 1u + perm(iy + perm(iz)));
    let bba = perm(ix + 1u + perm(iy + 1u + perm(iz)));
    let bab = perm(ix + 1u + perm(iy + perm(iz + 1u)));
    let bbb = perm(ix + 1u + perm(iy + 1u + perm(iz + 1u)));
    
    // Blend results from the 8 corners
    let x1 = lerp(grad3(aaa, pf.x, pf.y, pf.z), grad3(baa, pf.x - 1.0, pf.y, pf.z), u);
    let x2 = lerp(grad3(aba, pf.x, pf.y - 1.0, pf.z), grad3(bba, pf.x - 1.0, pf.y - 1.0, pf.z), u);
    let x3 = lerp(grad3(aab, pf.x, pf.y, pf.z - 1.0), grad3(bab, pf.x - 1.0, pf.y, pf.z - 1.0), u);
    let x4 = lerp(grad3(abb, pf.x, pf.y - 1.0, pf.z - 1.0), grad3(bbb, pf.x - 1.0, pf.y - 1.0, pf.z - 1.0), u);
    
    let y1 = lerp(x1, x2, v);
    let y2 = lerp(x3, x4, v);
    
    return lerp(y1, y2, w);
}
