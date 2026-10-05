//! Motion-aware visual inspection: where to look, what the engine can measure there.
//!
//! Mechanical measurements (exact, from evaluated layer state and rendered pixels) are kept
//! separate from judgment calls. Anything heuristic is labelled `Basis::Approximate`. Nothing here
//! decides whether a design is *good*; it finds what a reviewer should look at and what can be
//! proven wrong without looking.

use std::collections::BTreeSet;

use anyhow::Result;
use image::{imageops, Rgba, RgbaImage};
use serde::Serialize;

use crate::agent_contract::{Basis, Diagnostic, Location, Severity};
use crate::schema::Manifest;
use crate::timeline::{evaluate_manifest_layers_at_frame, LayerDebugState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    Moving,
    Still,
}

#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub kind: RunKind,
    pub start_frame: u32,
    pub end_frame: u32,
    /// entrance | hold | transition | exit | ending | motion
    pub phase: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Timeline {
    pub total_frames: u32,
    pub fps: u32,
    pub first_visible_frame: Option<u32>,
    pub last_visible_frame: Option<u32>,
    pub runs: Vec<Run>,
    pub basis: &'static str,
}

fn state_signature(states: &[LayerDebugState]) -> Vec<(bool, [f32; 6])> {
    states
        .iter()
        .map(|s| {
            (
                s.visible && s.opacity > 0.0,
                [
                    s.position.x,
                    s.position.y,
                    s.scale.x,
                    s.scale.y,
                    s.rotation_degrees,
                    s.opacity,
                ],
            )
        })
        .collect()
}

fn differs(a: &[(bool, [f32; 6])], b: &[(bool, [f32; 6])]) -> bool {
    a.iter().zip(b).any(|((va, xa), (vb, xb))| {
        va != vb || xa.iter().zip(xb).any(|(p, q)| (p - q).abs() > 1e-4)
    })
}

/// Evaluate every frame's layer state and split the timeline into moving/still runs.
pub fn build_timeline(manifest: &Manifest) -> Result<Timeline> {
    let total = manifest.environment.total_frames();
    let mut moving = vec![false; total as usize];
    let mut visible = vec![false; total as usize];
    let mut previous: Option<Vec<(bool, [f32; 6])>> = None;
    for frame in 0..total {
        let states = evaluate_manifest_layers_at_frame(manifest, frame)?;
        visible[frame as usize] = states.iter().any(|s| s.visible && s.opacity > 0.0);
        let signature = state_signature(&states);
        if let Some(prev) = &previous {
            moving[frame as usize] = differs(prev, &signature);
        }
        previous = Some(signature);
    }

    // A frame is "moving" when it differs from the previous one; frame 0 takes frame 1's status
    // so a lone first frame doesn't fragment the first run.
    if total > 1 {
        moving[0] = moving[1];
    }
    let mut runs: Vec<Run> = Vec::new();
    for frame in 0..total {
        let kind = if moving[frame as usize] {
            RunKind::Moving
        } else {
            RunKind::Still
        };
        match runs.last_mut() {
            Some(last) if last.kind == kind => last.end_frame = frame,
            _ => runs.push(Run {
                kind,
                start_frame: frame,
                end_frame: frame,
                phase: "motion",
            }),
        }
    }

    // Phase labels. First still run (>= 2 frames) after motion is the hold; motion before it is the
    // entrance; motion after the last hold is the exit; a trailing still run after an exit is the
    // ending.
    let first_hold = runs
        .iter()
        .position(|r| r.kind == RunKind::Still && r.end_frame - r.start_frame >= 1);
    let last_moving = runs.iter().rposition(|r| r.kind == RunKind::Moving);
    for (index, run) in runs.iter_mut().enumerate() {
        let moving_run = run.kind == RunKind::Moving;
        run.phase = match (moving_run, first_hold, last_moving) {
            (false, Some(hold), Some(lm)) if index > lm && index != hold => "ending",
            (false, Some(hold), _) if index == hold => "hold",
            (false, _, _) => "hold",
            (true, Some(hold), _) if index < hold => "entrance",
            (true, Some(_), Some(lm)) if index == lm => "exit",
            (true, Some(_), _) => "transition",
            (true, None, _) => "motion",
        };
    }
    // A trailing still run after the last moving run is the ending only when an exit preceded it.
    if let (Some(lm), Some(hold)) = (last_moving, first_hold) {
        if runs[lm].phase == "exit" && lm > hold {
            if let Some(last) = runs.last_mut() {
                if last.kind == RunKind::Still {
                    last.phase = "ending";
                }
            }
        } else if lm < hold {
            // No motion after the first hold: the last moving run is an entrance, not an exit.
            runs[lm].phase = "entrance";
        }
    }

    Ok(Timeline {
        total_frames: total,
        fps: manifest.environment.fps,
        first_visible_frame: visible.iter().position(|v| *v).map(|f| f as u32),
        last_visible_frame: visible.iter().rposition(|v| *v).map(|f| f as u32),
        runs,
        basis: "evaluated layer state (position, scale, rotation, opacity, visibility)",
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct SamplePoint {
    pub frame: u32,
    pub time_seconds: f64,
    /// `frame/fps` as an exact fraction.
    pub time_rational: String,
    pub phase: &'static str,
    pub roles: Vec<String>,
}

/// Choose frames: first, last, start/middle/end of every run, topped up with evenly spaced frames
/// so changes the layer-state profile cannot see (colour/text/sequence content) are still covered.
pub fn plan_samples(timeline: &Timeline, max_samples: usize) -> Vec<SamplePoint> {
    let mut picks: Vec<(u32, String)> = Vec::new();
    let last_frame = timeline.total_frames.saturating_sub(1);
    picks.push((0, "first".to_owned()));
    for run in &timeline.runs {
        let label = run.phase;
        picks.push((run.start_frame, format!("{label}_start")));
        if run.end_frame > run.start_frame + 1 {
            picks.push((
                (run.start_frame + run.end_frame) / 2,
                format!("{label}_mid"),
            ));
        }
        picks.push((run.end_frame, format!("{label}_end")));
    }
    picks.push((last_frame, "last".to_owned()));

    // Merge roles per frame.
    let mut frames: Vec<(u32, Vec<String>)> = Vec::new();
    for (frame, role) in picks {
        match frames.iter_mut().find(|(f, _)| *f == frame) {
            Some((_, roles)) => roles.push(role),
            None => frames.push((frame, vec![role])),
        }
    }
    // Prune to budget: keep first/last and run boundaries preferentially (drop `_mid` first).
    let priority = |roles: &[String]| -> u8 {
        if roles.iter().any(|r| r == "first" || r == "last") {
            0
        } else if roles.iter().any(|r| !r.ends_with("_mid")) {
            1
        } else {
            2
        }
    };
    while frames.len() > max_samples.max(2) {
        let drop = frames
            .iter()
            .enumerate()
            .filter(|(_, (_, roles))| priority(roles) > 0)
            .max_by_key(|(i, (_, roles))| (priority(roles), usize::MAX - *i))
            .map(|(i, _)| i);
        match drop {
            Some(i) => {
                frames.remove(i);
            }
            None => break,
        }
    }
    // Fill with even samples if under budget and the timeline is longer than the pick set.
    let have: BTreeSet<u32> = frames.iter().map(|(f, _)| *f).collect();
    let spare = max_samples.saturating_sub(frames.len()).min(3);
    for k in 1..=spare {
        let frame = (u64::from(last_frame) * k as u64 / (spare as u64 + 1)) as u32;
        if !have.contains(&frame) && frames.iter().all(|(f, _)| *f != frame) {
            frames.push((frame, vec!["even".to_owned()]));
        }
    }
    frames.sort_by_key(|(f, _)| *f);

    frames
        .into_iter()
        .map(|(frame, roles)| {
            let phase = timeline
                .runs
                .iter()
                .find(|r| r.start_frame <= frame && frame <= r.end_frame)
                .map(|r| r.phase)
                .unwrap_or("motion");
            SamplePoint {
                frame,
                time_seconds: f64::from(frame) / f64::from(timeline.fps),
                time_rational: format!("{frame}/{}", timeline.fps),
                phase,
                roles,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Bbox {
    pub x0: u32,
    pub y0: u32,
    /// Inclusive.
    pub x1: u32,
    pub y1: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq)]
pub struct EdgeTouch {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl EdgeTouch {
    pub fn any(&self) -> bool {
        self.left || self.right || self.top || self.bottom
    }
    pub fn sides(&self) -> Vec<&'static str> {
        let mut sides = Vec::new();
        if self.left {
            sides.push("left");
        }
        if self.right {
            sides.push("right");
        }
        if self.top {
            sides.push("top");
        }
        if self.bottom {
            sides.push("bottom");
        }
        sides
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PixelStats {
    pub width: u32,
    pub height: u32,
    pub alpha_bbox: Option<Bbox>,
    /// Contact with the canvas edge, within a thin tolerance band (see `edge_tolerance_px`).
    pub edge_touch: EdgeTouch,
    pub edge_tolerance_px: [u32; 2],
    /// Fraction of pixels with alpha > 0.
    pub covered_fraction: f64,
    pub has_transparency: bool,
}

/// Measure a full-resolution RGBA frame. `alpha_threshold`: alpha above this counts as content.
pub fn pixel_stats(rgba: &[u8], width: u32, height: u32) -> PixelStats {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    let mut covered = 0u64;
    let mut transparent = false;
    for (index, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let alpha = pixel[3];
        if alpha < 255 {
            transparent = true;
        }
        if alpha > 0 {
            covered += 1;
            let x = index as u32 % width;
            let y = index as u32 / width;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    let bbox = (covered > 0).then_some(Bbox { x0, y0, x1, y1 });
    // Content cut by the canvas edge need not light the last column (a glyph stroke gap can sit
    // exactly on the cut), so "touching" means within a thin band: max(4px, 0.5% of the side).
    let tol_x = ((f64::from(width) * 0.005).round() as u32).max(4);
    let tol_y = ((f64::from(height) * 0.005).round() as u32).max(4);
    let edge_touch = bbox
        .map(|b| EdgeTouch {
            left: b.x0 < tol_x,
            right: b.x1 + tol_x >= width,
            top: b.y0 < tol_y,
            bottom: b.y1 + tol_y >= height,
        })
        .unwrap_or_default();
    PixelStats {
        width,
        height,
        alpha_bbox: bbox,
        edge_touch,
        edge_tolerance_px: [tol_x, tol_y],
        covered_fraction: covered as f64 / (f64::from(width) * f64::from(height)),
        has_transparency: transparent,
    }
}

/// Smooth downscale of an RGBA buffer (premultiplication-agnostic; used only for review images).
pub fn downscale_rgba(rgba: Vec<u8>, width: u32, height: u32, new_w: u32, new_h: u32) -> Vec<u8> {
    if (width, height) == (new_w, new_h) {
        return rgba;
    }
    let Some(img) = RgbaImage::from_raw(width, height, rgba) else {
        return Vec::new();
    };
    imageops::resize(&img, new_w, new_h, imageops::FilterType::Triangle).into_raw()
}

// ───────────────────────────── diagnostics ─────────────────────────────

pub struct DiagnosticConfig {
    /// Fraction of each canvas edge reserved as margin (title-safe ≈ 0.05).
    pub safe_margin: f64,
    pub fps: u32,
}

pub fn timeline_diagnostics(timeline: &Timeline, config: &DiagnosticConfig) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    match (timeline.first_visible_frame, timeline.last_visible_frame) {
        (None, _) | (_, None) => out.push(
            Diagnostic::new(
                Severity::Error,
                "inspect",
                "timing.nothing_visible",
                "no layer is visible with opacity > 0 on any frame",
            )
            .basis(Basis::Exact)
            .recover("Check layer start_time/end_time, opacity, and keyframe ranges."),
        ),
        (Some(first), Some(last)) => {
            if first > 0 {
                out.push(
                    Diagnostic::new(
                        Severity::Info,
                        "inspect",
                        "timing.leading_empty",
                        format!(
                            "nothing is visible until frame {first} ({:.3}s)",
                            f64::from(first) / f64::from(config.fps)
                        ),
                    )
                    .basis(Basis::Exact),
                );
            }
            if last + 1 < timeline.total_frames {
                out.push(
                    Diagnostic::new(
                        Severity::Info,
                        "inspect",
                        "timing.ends_empty",
                        format!(
                            "nothing is visible after frame {last}; the last {} frame(s) are empty (intended for transparent exports with an exit)",
                            timeline.total_frames - 1 - last
                        ),
                    )
                    .basis(Basis::Exact),
                );
            }
        }
    }
    // Animation still moving on the final frame: the settle/exit is cut off.
    if let Some(run) = timeline.runs.last() {
        if run.kind == RunKind::Moving
            && run.end_frame == timeline.total_frames - 1
            && timeline.total_frames > 1
        {
            out.push(
                Diagnostic::new(
                    Severity::Warning,
                    "inspect",
                    "timing.motion_on_last_frame",
                    "layer state is still changing on the last frame; the animation is cut off rather than settled",
                )
                .basis(Basis::Exact)
                .recover("Lengthen the duration or end the animation earlier (keyframe end_frame / layer end_time)."),
            );
        }
    }
    // Holds shorter than 0.75 s (heuristic).
    let min_hold = (0.75 * f64::from(config.fps)).ceil() as u32;
    for run in &timeline.runs {
        if run.phase == "hold" && run.end_frame >= 1 {
            let length = run.end_frame - run.start_frame + 1;
            let content_visible = timeline
                .first_visible_frame
                .zip(timeline.last_visible_frame)
                .is_some_and(|(a, b)| run.start_frame >= a && run.end_frame <= b);
            if content_visible && length < min_hold {
                out.push(
                    Diagnostic::new(
                        Severity::Warning,
                        "inspect",
                        "timing.hold_short",
                        format!(
                            "static hold of {length} frame(s) ({:.2}s) between frames {}..{} may be too short to read",
                            f64::from(length) / f64::from(config.fps),
                            run.start_frame,
                            run.end_frame
                        ),
                    )
                    .basis(Basis::Approximate)
                    .recover("Judgment call: 0.75s is a rule of thumb for short text; confirm against the brief."),
                );
            }
        }
    }
    out
}

/// Per-layer, per-sample bounds diagnostics from the layer rendered in isolation.
pub fn bounds_diagnostics(
    layer_id: &str,
    frame: u32,
    phase: &str,
    stats: &PixelStats,
    config: &DiagnosticConfig,
    precision_px: f64,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    // Entering/leaving the canvas is normal during motion; clipping at rest is the defect.
    let tol = stats.edge_tolerance_px[0].max(stats.edge_tolerance_px[1]);
    let moving = matches!(phase, "entrance" | "exit" | "transition" | "motion");
    let location = || Location::layer(layer_id.to_owned());
    let Some(bbox) = stats.alpha_bbox else {
        out.push(
            Diagnostic::new(
                Severity::Warning,
                "inspect",
                "layer.renders_nothing",
                format!("layer '{layer_id}' is visible by state at frame {frame} but draws no pixels inside the canvas"),
            )
            .at(location())
            .basis(Basis::Exact)
            .recover("Check position/anchor/scale: the layer may be positioned off-canvas, or fully transparent."),
        );
        return out;
    };
    let full_frame = stats.covered_fraction > 0.98;
    if stats.edge_touch.any() && !full_frame {
        out.push(
            Diagnostic::new(
                if moving { Severity::Info } else { Severity::Warning },
                "inspect",
                "layout.touches_canvas_edge",
                format!(
                    "layer '{layer_id}' content is within {tol}px of the canvas {} edge at frame {frame}; it is likely clipped or flush",
                    stats.edge_touch.sides().join("/")
                ),
            )
            .at(location())
            .basis(Basis::Exact)
            .observed(serde_json::json!({"bbox": bbox, "canvas": [stats.width, stats.height], "precision_px": precision_px}))
            .recover("Move or scale the layer inward, or shorten the content; verify with `vcr inspect` again."),
        );
    }
    if !full_frame {
        let margin_x = (f64::from(stats.width) * config.safe_margin).round() as u32;
        let margin_y = (f64::from(stats.height) * config.safe_margin).round() as u32;
        let outside = bbox.x0 < margin_x
            || bbox.y0 < margin_y
            || bbox.x1 + margin_x >= stats.width
            || bbox.y1 + margin_y >= stats.height;
        if outside && !stats.edge_touch.any() && !moving {
            out.push(
                Diagnostic::new(
                    Severity::Info,
                    "inspect",
                    "layout.outside_safe_area",
                    format!(
                        "layer '{layer_id}' extends into the {:.0}% safe margin at frame {frame}",
                        config.safe_margin * 100.0
                    ),
                )
                .at(location())
                .basis(Basis::Exact)
                .observed(serde_json::json!({"bbox": bbox, "margin_px": [margin_x, margin_y]}))
                .recover("Policy check, not an error: broadcast title-safe is ~5% per side."),
            );
        }
    }
    out
}

// ───────────────────────────── contact sheet ─────────────────────────────

const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

fn draw_digits(img: &mut RgbaImage, text: &str, x: u32, y: u32, scale: u32) {
    let mut cursor = x;
    for ch in text.chars() {
        let glyph: Option<[u8; 5]> = match ch {
            '0'..='9' => Some(DIGITS[ch as usize - '0' as usize]),
            'f' => Some([0b011, 0b100, 0b110, 0b100, 0b100]),
            's' => Some([0b011, 0b100, 0b010, 0b001, 0b110]),
            '.' => Some([0, 0, 0, 0, 0b010]),
            _ => None,
        };
        if let Some(rows) = glyph {
            for (ry, row) in rows.iter().enumerate() {
                for rx in 0..3u32 {
                    if row & (0b100 >> rx) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                let px = cursor + rx * scale + dx;
                                let py = y + ry as u32 * scale + dy;
                                if px < img.width() && py < img.height() {
                                    img.put_pixel(px, py, Rgba([255, 255, 255, 255]));
                                }
                            }
                        }
                    }
                }
            }
        }
        cursor += 4 * scale;
    }
}

/// Grid of tiles over a checkerboard (so transparency is visible), each labelled `f<frame> <s>s`.
pub fn contact_sheet(tiles: &[(RgbaImage, u32, f64)], columns: u32, tile_width: u32) -> RgbaImage {
    let columns = columns.max(1).min(tiles.len().max(1) as u32);
    let rows = (tiles.len() as u32).div_ceil(columns).max(1);
    let (src_w, src_h) = tiles
        .first()
        .map(|(img, _, _)| (img.width(), img.height()))
        .unwrap_or((tile_width, tile_width));
    let tile_height =
        ((u64::from(tile_width) * u64::from(src_h)) / u64::from(src_w.max(1))).max(1) as u32;
    let label_h = 16;
    let gap = 6;
    let sheet_w = columns * (tile_width + gap) + gap;
    let sheet_h = rows * (tile_height + label_h + gap) + gap;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba([24, 24, 28, 255]));
    for (i, (tile, frame, seconds)) in tiles.iter().enumerate() {
        let col = i as u32 % columns;
        let row = i as u32 / columns;
        let ox = gap + col * (tile_width + gap);
        let oy = gap + row * (tile_height + label_h + gap);
        let resized = imageops::resize(
            tile,
            tile_width,
            tile_height,
            imageops::FilterType::Triangle,
        );
        for y in 0..tile_height {
            for x in 0..tile_width {
                let checker = if ((x / 8) + (y / 8)) % 2 == 0 {
                    52u8
                } else {
                    78u8
                };
                let p = resized.get_pixel(x, y).0;
                let a = u32::from(p[3]);
                let blend =
                    |c: u8, b: u8| ((u32::from(c) * a + u32::from(b) * (255 - a)) / 255) as u8;
                sheet.put_pixel(
                    ox + x,
                    oy + label_h + y,
                    Rgba([
                        blend(p[0], checker),
                        blend(p[1], checker),
                        blend(p[2], checker),
                        255,
                    ]),
                );
            }
        }
        draw_digits(
            &mut sheet,
            &format!("f{frame} {seconds:.2}s"),
            ox + 2,
            oy + 4,
            2,
        );
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_stats_find_bbox_and_edge_touch() {
        let (w, h) = (40u32, 30u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 10..30 {
            for x in 10..20 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i + 3] = 255;
            }
        }
        let stats = pixel_stats(&rgba, w, h);
        assert_eq!(
            stats.alpha_bbox,
            Some(Bbox {
                x0: 10,
                y0: 10,
                x1: 19,
                y1: 29
            })
        );
        assert!(stats.edge_touch.bottom && !stats.edge_touch.left && !stats.edge_touch.top);
        assert_eq!(stats.edge_tolerance_px, [4, 4]);
        assert!(stats.has_transparency);
    }

    #[test]
    fn contact_sheet_dimensions_scale_with_tiles() {
        let tile = RgbaImage::from_pixel(64, 36, Rgba([255, 0, 0, 128]));
        let tiles = vec![
            (tile.clone(), 0, 0.0),
            (tile.clone(), 5, 0.5),
            (tile, 9, 0.9),
        ];
        let sheet = contact_sheet(&tiles, 2, 64);
        assert_eq!(sheet.width(), 2 * (64 + 6) + 6);
        assert!(sheet.height() > 36 * 2);
    }
}
