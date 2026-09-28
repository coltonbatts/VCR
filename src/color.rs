//! Canonical color pipeline shared by the GPU and software backends.
//!
//! See `docs/COLOR_PIPELINE.md`. In short:
//!
//! 1. Inputs (manifest colors, image files, rasterized text/ascii, custom shader output) are
//!    straight-alpha, sRGB-encoded.
//! 2. They are decoded to linear light and premultiplied by alpha.
//! 3. Layer textures store premultiplied linear values sRGB-encoded in 8 bits per channel
//!    (exactly what an `Rgba8UnormSrgb` texture holds); filtering happens after decoding.
//! 4. Layers composite with premultiplied source-over in linear light, accumulated in float.
//! 5. The final pixel is unpremultiplied and sRGB-encoded exactly once, at output.

use std::sync::OnceLock;

use crate::schema::ColorRgba;

/// Identifier recorded in render metadata so outputs can be tied to a pipeline revision.
pub const PIPELINE_ID: &str = "linear-light premultiplied, srgb-encoded 8-bit texels, srgb output";

/// Output pixels whose alpha quantizes to 0 are written as fully transparent black.
const TRANSPARENT_ALPHA_THRESHOLD: f32 = 0.5 / 255.0;

/// sRGB EOTF (IEC 61966-2-1): encoded value -> linear light.
pub fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Inverse sRGB EOTF: linear light -> encoded value.
pub fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// Round-to-nearest unorm8 quantization (matches GPU float -> unorm conversion).
pub fn unorm8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn srgb8_decode_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut lut = [0.0_f32; 256];
        for (index, entry) in lut.iter_mut().enumerate() {
            *entry = srgb_to_linear(index as f32 / 255.0);
        }
        lut
    })
}

/// Linear value of an sRGB-encoded 8-bit channel.
pub fn srgb8_to_linear(value: u8) -> f32 {
    srgb8_decode_lut()[value as usize]
}

/// Authored straight-alpha sRGB color -> premultiplied linear.
pub fn premultiplied_linear(color: ColorRgba) -> [f32; 4] {
    let alpha = color.a.clamp(0.0, 1.0);
    [
        srgb_to_linear(color.r.clamp(0.0, 1.0)) * alpha,
        srgb_to_linear(color.g.clamp(0.0, 1.0)) * alpha,
        srgb_to_linear(color.b.clamp(0.0, 1.0)) * alpha,
        alpha,
    ]
}

/// Premultiplied linear -> texel storage (sRGB-encoded premultiplied, 8 bits per channel).
pub fn encode_texel(premultiplied: [f32; 4]) -> [u8; 4] {
    [
        unorm8(linear_to_srgb(premultiplied[0])),
        unorm8(linear_to_srgb(premultiplied[1])),
        unorm8(linear_to_srgb(premultiplied[2])),
        unorm8(premultiplied[3]),
    ]
}

/// Texel storage -> premultiplied linear.
pub fn decode_texel(texel: [u8; 4]) -> [f32; 4] {
    let lut = srgb8_decode_lut();
    [
        lut[texel[0] as usize],
        lut[texel[1] as usize],
        lut[texel[2] as usize],
        texel[3] as f32 / 255.0,
    ]
}

/// Premultiplied linear accumulator -> straight-alpha sRGB RGBA8 output pixel.
pub fn encode_output(premultiplied: [f32; 4]) -> [u8; 4] {
    let alpha = premultiplied[3].clamp(0.0, 1.0);
    if alpha < TRANSPARENT_ALPHA_THRESHOLD {
        return [0, 0, 0, 0];
    }
    let channel = |value: f32| unorm8(linear_to_srgb((value / alpha).clamp(0.0, 1.0)));
    [
        channel(premultiplied[0]),
        channel(premultiplied[1]),
        channel(premultiplied[2]),
        unorm8(alpha),
    ]
}

/// Straight-alpha sRGB RGBA8 (decoded image files, rasterized text) -> texel storage, in place.
pub fn straight_srgb8_to_texels(rgba: &mut [u8]) {
    let lut = srgb8_decode_lut();
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = pixel[3] as f32 / 255.0;
        let texel = encode_texel([
            lut[pixel[0] as usize] * alpha,
            lut[pixel[1] as usize] * alpha,
            lut[pixel[2] as usize] * alpha,
            alpha,
        ]);
        pixel.copy_from_slice(&texel);
    }
}

/// RGBA8 premultiplied in sRGB space (tiny-skia pixmaps) -> texel storage, in place.
pub fn srgb_premultiplied8_to_texels(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha8 = pixel[3];
        if alpha8 == 0 {
            pixel.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let alpha = alpha8 as f32 / 255.0;
        let straight = |value: u8| (value as f32 / alpha8 as f32).min(1.0);
        let texel = encode_texel([
            srgb_to_linear(straight(pixel[0])) * alpha,
            srgb_to_linear(straight(pixel[1])) * alpha,
            srgb_to_linear(straight(pixel[2])) * alpha,
            alpha,
        ]);
        pixel.copy_from_slice(&texel);
    }
}

/// Premultiplied source-over in linear light.
pub fn source_over(destination: &mut [f32; 4], source: [f32; 4]) {
    let inverse_alpha = 1.0 - source[3];
    for channel in 0..4 {
        destination[channel] = source[channel] + destination[channel] * inverse_alpha;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_transfer_round_trips_every_code_value() {
        for code in 0..=255_u8 {
            let linear = srgb8_to_linear(code);
            assert_eq!(unorm8(linear_to_srgb(linear)), code, "code {code}");
        }
    }

    #[test]
    fn opaque_texels_round_trip_through_output_encode() {
        for code in [0_u8, 1, 17, 64, 128, 188, 254, 255] {
            let mut rgba = [code, 255 - code, code / 2, 255];
            straight_srgb8_to_texels(&mut rgba);
            let out = encode_output(decode_texel(rgba));
            assert_eq!(out, [code, 255 - code, code / 2, 255]);
        }
    }

    #[test]
    fn half_opacity_white_over_black_blends_in_linear_light() {
        // Linear-light 50% mix of white and black is 0.5 linear == sRGB 188, not 128.
        let mut destination = premultiplied_linear(ColorRgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        let white = premultiplied_linear(ColorRgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.5,
        });
        source_over(&mut destination, white);
        assert_eq!(encode_output(destination), [188, 188, 188, 255]);
    }

    #[test]
    fn translucent_color_over_transparent_keeps_straight_color() {
        let mut destination = [0.0; 4];
        source_over(
            &mut destination,
            premultiplied_linear(ColorRgba {
                r: 1.0,
                g: 0.5,
                b: 0.0,
                a: 0.25,
            }),
        );
        // Output is straight alpha: the color survives unchanged, alpha is 25%.
        assert_eq!(encode_output(destination), [255, 128, 0, 64]);
    }

    #[test]
    fn fully_transparent_output_is_zeroed() {
        assert_eq!(encode_output([0.001, 0.0, 0.0, 0.0005]), [0, 0, 0, 0]);
    }
}
