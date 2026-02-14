// 3D Simplex Noise for WGSL
// Based on Stefan Gustavson's implementation (public domain)
// Adapted for WGSL by VCR Standard Library

// Permutation table
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

fn grad3(hash: u32, x: f32, y: f32, z: f32) -> f32 {
    let h = hash & 15u;
    let u = select(y, x, h < 8u);
    let v = select(select(x, z, h == 12u || h == 14u), y, h < 4u);
    return select(u, -u, (h & 1u) == 0u) + select(v, -v, (h & 2u) == 0u);
}

// 3D Simplex Noise
fn simplex_noise_3d(p: vec3<f32>) -> f32 {
    // Skewing and unskewing factors
    let F3 = 1.0 / 3.0;
    let G3 = 1.0 / 6.0;
    
    // Skew the input space to determine which simplex cell we're in
    let s = (p.x + p.y + p.z) * F3;
    let i = floor(p.x + s);
    let j = floor(p.y + s);
    let k = floor(p.z + s);
    
    let t = (i + j + k) * G3;
    let X0 = i - t;
    let Y0 = j - t;
    let Z0 = k - t;
    let x0 = p.x - X0;
    let y0 = p.y - Y0;
    let z0 = p.z - Z0;
    
    // Determine which simplex we are in
    var i1: f32;
    var j1: f32;
    var k1: f32;
    var i2: f32;
    var j2: f32;
    var k2: f32;
    
    if (x0 >= y0) {
        if (y0 >= z0) {
            i1 = 1.0; j1 = 0.0; k1 = 0.0; i2 = 1.0; j2 = 1.0; k2 = 0.0;
        } else if (x0 >= z0) {
            i1 = 1.0; j1 = 0.0; k1 = 0.0; i2 = 1.0; j2 = 0.0; k2 = 1.0;
        } else {
            i1 = 0.0; j1 = 0.0; k1 = 1.0; i2 = 1.0; j2 = 0.0; k2 = 1.0;
        }
    } else {
        if (y0 < z0) {
            i1 = 0.0; j1 = 0.0; k1 = 1.0; i2 = 0.0; j2 = 1.0; k2 = 1.0;
        } else if (x0 < z0) {
            i1 = 0.0; j1 = 1.0; k1 = 0.0; i2 = 0.0; j2 = 1.0; k2 = 1.0;
        } else {
            i1 = 0.0; j1 = 1.0; k1 = 0.0; i2 = 1.0; j2 = 1.0; k2 = 0.0;
        }
    }
    
    let x1 = x0 - i1 + G3;
    let y1 = y0 - j1 + G3;
    let z1 = z0 - k1 + G3;
    let x2 = x0 - i2 + 2.0 * G3;
    let y2 = y0 - j2 + 2.0 * G3;
    let z2 = z0 - k2 + 2.0 * G3;
    let x3 = x0 - 1.0 + 3.0 * G3;
    let y3 = y0 - 1.0 + 3.0 * G3;
    let z3 = z0 - 1.0 + 3.0 * G3;
    
    // Work out the hashed gradient indices
    let ii = u32(i) & 255u;
    let jj = u32(j) & 255u;
    let kk = u32(k) & 255u;
    
    let gi0 = perm(ii + perm(jj + perm(kk)));
    let gi1 = perm(ii + u32(i1) + perm(jj + u32(j1) + perm(kk + u32(k1))));
    let gi2 = perm(ii + u32(i2) + perm(jj + u32(j2) + perm(kk + u32(k2))));
    let gi3 = perm(ii + 1u + perm(jj + 1u + perm(kk + 1u)));
    
    // Calculate the contribution from the four corners
    var n0: f32;
    var n1: f32;
    var n2: f32;
    var n3: f32;
    
    var t0 = 0.6 - x0 * x0 - y0 * y0 - z0 * z0;
    if (t0 < 0.0) {
        n0 = 0.0;
    } else {
        t0 *= t0;
        n0 = t0 * t0 * grad3(gi0, x0, y0, z0);
    }
    
    var t1 = 0.6 - x1 * x1 - y1 * y1 - z1 * z1;
    if (t1 < 0.0) {
        n1 = 0.0;
    } else {
        t1 *= t1;
        n1 = t1 * t1 * grad3(gi1, x1, y1, z1);
    }
    
    var t2 = 0.6 - x2 * x2 - y2 * y2 - z2 * z2;
    if (t2 < 0.0) {
        n2 = 0.0;
    } else {
        t2 *= t2;
        n2 = t2 * t2 * grad3(gi2, x2, y2, z2);
    }
    
    var t3 = 0.6 - x3 * x3 - y3 * y3 - z3 * z3;
    if (t3 < 0.0) {
        n3 = 0.0;
    } else {
        t3 *= t3;
        n3 = t3 * t3 * grad3(gi3, x3, y3, z3);
    }
    
    // Add contributions from each corner to get the final noise value
    // The result is scaled to return values in the interval [-1,1]
    return 32.0 * (n0 + n1 + n2 + n3);
}

// 2D Simplex Noise (simplified from 3D)
fn simplex_noise_2d(p: vec2<f32>) -> f32 {
    return simplex_noise_3d(vec3<f32>(p.x, p.y, 0.0));
}
