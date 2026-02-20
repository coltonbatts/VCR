//! Seed-locked in-place channel displacement for flat RGBA frame buffers.
//!
//! This effect is designed for deterministic, offline rendering:
//! - no external RNG crate
//! - integer-only math
//! - no allocations
//! - alpha channel is never modified

use std::error::Error;
use std::fmt::{Display, Formatter};

/// Tiny deterministic PRNG (xorshift64*).
///
/// This is fully self-contained and uses only integer operations.
#[derive(Debug, Clone, Copy)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// Build a deterministic RNG from a 64-bit seed.
    ///
    /// `seed = 0` is remapped to a non-zero internal state so the generator
    /// cannot lock into an all-zero sequence.
    pub const fn from_seed(seed: u64) -> Self {
        let mixed = seed ^ 0x9E37_79B9_7F4A_7C15;
        let state = if mixed == 0 {
            0xA076_1D64_78BD_642F
        } else {
            mixed
        };
        Self { state }
    }

    /// Next pseudo-random `u64`.
    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform value in `[0, max_inclusive]` using rejection sampling.
    #[inline(always)]
    pub fn next_bounded(&mut self, max_inclusive: usize) -> usize {
        if max_inclusive == 0 {
            return 0;
        }

        let bound = (max_inclusive as u64) + 1;
        let zone = u64::MAX - (u64::MAX % bound);
        loop {
            let sample = self.next_u64();
            if sample < zone {
                return (sample % bound) as usize;
            }
        }
    }
}

/// Configuration for a deterministic seed-locked channel displacement pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeedLockedChannelDisplacement {
    pub seed: u64,
    pub luma_threshold: u8,
    pub max_offset: usize,
}

/// Validation failures for in-place displacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelDisplacementError {
    DimensionsOverflow,
    BufferLengthMismatch { expected: usize, actual: usize },
}

impl Display for ChannelDisplacementError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DimensionsOverflow => write!(f, "frame dimensions overflowed usize"),
            Self::BufferLengthMismatch { expected, actual } => write!(
                f,
                "RGBA buffer length mismatch: expected {expected} bytes, got {actual} bytes"
            ),
        }
    }
}

impl Error for ChannelDisplacementError {}

/// Per-channel displacement vectors in pixel units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelOffsets {
    pub red_dx: isize,
    pub red_dy: isize,
    pub blue_dx: isize,
    pub blue_dy: isize,
}

impl SeedLockedChannelDisplacement {
    /// Sample deterministic channel offsets from the configured seed.
    ///
    /// Red and blue offsets are independent magnitudes, but share traversal
    /// direction signs to guarantee source reads remain valid during in-place
    /// writes with no secondary buffer.
    pub fn offsets_for_frame(&self, width: usize, height: usize) -> ChannelOffsets {
        if self.max_offset == 0 || width == 0 || height == 0 {
            return ChannelOffsets {
                red_dx: 0,
                red_dy: 0,
                blue_dx: 0,
                blue_dy: 0,
            };
        }

        let max_x = self
            .max_offset
            .min(width.saturating_sub(1))
            .min(isize::MAX as usize);
        let max_y = self
            .max_offset
            .min(height.saturating_sub(1))
            .min(isize::MAX as usize);
        let mut rng = XorShift64::from_seed(self.seed);

        let x_forward = (rng.next_u64() & 1) == 0;
        let y_forward = (rng.next_u64() & 1) == 0;

        let red_dx = signed_offset(&mut rng, max_x, x_forward);
        let red_dy = signed_offset(&mut rng, max_y, y_forward);
        let blue_dx = signed_offset(&mut rng, max_x, x_forward);
        let blue_dy = signed_offset(&mut rng, max_y, y_forward);

        ChannelOffsets {
            red_dx,
            red_dy,
            blue_dx,
            blue_dy,
        }
    }

    /// Apply displacement in-place on a flat RGBA buffer.
    ///
    /// Alpha bytes are preserved exactly.
    pub fn apply(
        &self,
        rgba: &mut [u8],
        width: usize,
        height: usize,
    ) -> Result<(), ChannelDisplacementError> {
        let pixel_count = width
            .checked_mul(height)
            .ok_or(ChannelDisplacementError::DimensionsOverflow)?;
        let expected_len = pixel_count
            .checked_mul(4)
            .ok_or(ChannelDisplacementError::DimensionsOverflow)?;

        if rgba.len() != expected_len {
            return Err(ChannelDisplacementError::BufferLengthMismatch {
                expected: expected_len,
                actual: rgba.len(),
            });
        }

        if width == 0 || height == 0 || self.max_offset == 0 {
            return Ok(());
        }

        let offsets = self.offsets_for_frame(width, height);
        if offsets.red_dx == 0
            && offsets.red_dy == 0
            && offsets.blue_dx == 0
            && offsets.blue_dy == 0
        {
            return Ok(());
        }

        let x_forward = offsets.red_dx >= 0 && offsets.blue_dx >= 0;
        let y_forward = offsets.red_dy >= 0 && offsets.blue_dy >= 0;
        process_in_place(
            rgba,
            width,
            height,
            self.luma_threshold,
            offsets,
            x_forward,
            y_forward,
        );

        Ok(())
    }
}

#[inline(always)]
fn signed_offset(rng: &mut XorShift64, max_inclusive: usize, forward: bool) -> isize {
    let magnitude = rng.next_bounded(max_inclusive) as isize;
    if forward {
        magnitude
    } else {
        -magnitude
    }
}

#[inline(always)]
fn bt709_luma_u8(r: u8, g: u8, b: u8) -> u8 {
    ((u16::from(r) * 54 + u16::from(g) * 183 + u16::from(b) * 19) >> 8) as u8
}

#[inline(always)]
fn offset_coord(coord: usize, delta: isize, len: usize) -> usize {
    debug_assert!(len > 0);
    let max_index = len - 1;
    if delta >= 0 {
        coord.saturating_add(delta as usize).min(max_index)
    } else {
        coord.saturating_sub(delta.unsigned_abs())
    }
}

fn process_in_place(
    rgba: &mut [u8],
    width: usize,
    height: usize,
    luma_threshold: u8,
    offsets: ChannelOffsets,
    x_forward: bool,
    y_forward: bool,
) {
    let mut y = if y_forward { 0 } else { height };
    while if y_forward { y < height } else { y > 0 } {
        if !y_forward {
            y -= 1;
        }
        let y_usize = y;

        let mut x = if x_forward { 0 } else { width };
        while if x_forward { x < width } else { x > 0 } {
            if !x_forward {
                x -= 1;
            }
            let x_usize = x;
            let dst_idx = ((y_usize * width) + x_usize) << 2;

            let luma = bt709_luma_u8(rgba[dst_idx], rgba[dst_idx + 1], rgba[dst_idx + 2]);
            if luma > luma_threshold {
                let src_red_x = offset_coord(x_usize, offsets.red_dx, width);
                let src_red_y = offset_coord(y_usize, offsets.red_dy, height);
                let src_blue_x = offset_coord(x_usize, offsets.blue_dx, width);
                let src_blue_y = offset_coord(y_usize, offsets.blue_dy, height);

                let src_red_idx = ((src_red_y * width) + src_red_x) << 2;
                let src_blue_idx = ((src_blue_y * width) + src_blue_x) << 2;

                // Update R/B only. Alpha is never touched.
                rgba[dst_idx] = rgba[src_red_idx];
                rgba[dst_idx + 2] = rgba[src_blue_idx + 2];
            }

            if x_forward {
                x += 1;
            }
        }

        if y_forward {
            y += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_frame(width: usize, height: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                out.push(((x * 17 + y * 11) & 255) as u8);
                out.push(((x * 7 + y * 19) & 255) as u8);
                out.push(((x * 23 + y * 3) & 255) as u8);
                out.push(((x * 29 + y * 13) & 255) as u8);
            }
        }
        out
    }

    #[test]
    fn apply_preserves_alpha_channel_exactly() {
        let width = 16;
        let height = 9;
        let mut frame = make_test_frame(width, height);
        let alpha_before: Vec<u8> = frame.chunks_exact(4).map(|px| px[3]).collect();

        let node = SeedLockedChannelDisplacement {
            seed: 0xDEAD_BEEF_CAFE_BABE,
            luma_threshold: 32,
            max_offset: 4,
        };
        node.apply(&mut frame, width, height)
            .expect("displacement should succeed");

        let alpha_after: Vec<u8> = frame.chunks_exact(4).map(|px| px[3]).collect();
        assert_eq!(alpha_before, alpha_after);
    }

    #[test]
    fn apply_is_deterministic_for_same_seed() {
        let width = 32;
        let height = 18;
        let source = make_test_frame(width, height);

        let mut a = source.clone();
        let mut b = source.clone();
        let node = SeedLockedChannelDisplacement {
            seed: 42,
            luma_threshold: 24,
            max_offset: 6,
        };

        node.apply(&mut a, width, height)
            .expect("first displacement should succeed");
        node.apply(&mut b, width, height)
            .expect("second displacement should succeed");

        assert_eq!(a, b, "same seed must produce byte-identical output");
    }

    #[test]
    fn threshold_can_disable_effect() {
        let width = 8;
        let height = 4;
        let mut frame = vec![10_u8; width * height * 4];
        for px in frame.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let baseline = frame.clone();

        let node = SeedLockedChannelDisplacement {
            seed: 7,
            luma_threshold: 250,
            max_offset: 8,
        };
        node.apply(&mut frame, width, height)
            .expect("displacement should succeed");

        assert_eq!(frame, baseline, "all pixels should remain unchanged");
    }

    #[test]
    fn apply_rejects_invalid_buffer_length() {
        let mut frame = vec![0_u8; 15];
        let node = SeedLockedChannelDisplacement {
            seed: 1,
            luma_threshold: 0,
            max_offset: 1,
        };

        let err = node
            .apply(&mut frame, 2, 2)
            .expect_err("buffer length mismatch should fail");
        assert!(matches!(
            err,
            ChannelDisplacementError::BufferLengthMismatch { .. }
        ));
    }
}
