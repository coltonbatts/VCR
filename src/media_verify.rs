//! Encoded-artifact verification: compare what is actually in the file with what was requested.
//!
//! Scope and limits are part of the result (`limits`), never implied:
//! - Container/stream facts come from `ffprobe`; frame count is the demuxed *packet* count
//!   (exact for intra-only codecs such as ProRes), not the container's advertised `nb_frames`.
//! - Frame rate is compared as an exact rational (`num/den`), never as a float.
//! - Duration is derived and compared within half a frame period; the exact rational duration of
//!   `frames/fps` is reported alongside the container's value.
//! - Transparency is measured on decoded RGBA pixels. An alpha-capable pix_fmt alone proves
//!   nothing, and an opaque result may be exactly what the brief wanted.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaCheck {
    pub id: &'static str,
    pub status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransparencyExpectation {
    /// Some decoded pixel in some frame must have alpha < 255.
    Required,
    /// Every decoded pixel must be fully opaque.
    None,
    /// Report only.
    #[default]
    Any,
}

impl std::str::FromStr for TransparencyExpectation {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "required" => Ok(Self::Required),
            "none" | "opaque" => Ok(Self::None),
            "any" => Ok(Self::Any),
            other => bail!("--expect-transparency must be required|none|any, got '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Expectations {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub frame_count: Option<u32>,
    pub container: Option<String>,
    pub codec: Option<String>,
    /// ffprobe profile token, e.g. `4444`, `HQ`.
    pub profile: Option<String>,
    pub alpha_capable: Option<bool>,
    pub transparency: TransparencyExpectation,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeFacts {
    pub container: String,
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub pix_fmt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub r_frame_rate: Option<String>,
    pub avg_frame_rate: Option<String>,
    pub advertised_nb_frames: Option<u64>,
    pub packet_count: Option<u64>,
    pub stream_duration_seconds: Option<f64>,
    pub format_duration_seconds: Option<f64>,
    pub color_space: Option<String>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_range: Option<String>,
    pub alpha_capable_pix_fmt: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransparencyStats {
    pub frames_decoded: u64,
    pub frames_with_transparency: u64,
    pub min_alpha: u8,
    pub transparent_pixel_fraction_max: f64,
    pub fully_transparent_pixel_fraction_max: f64,
    pub first_frame_min_alpha: Option<u8>,
    pub last_frame_min_alpha: Option<u8>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaReport {
    pub passed: bool,
    pub probe: ProbeFacts,
    pub checks: Vec<MediaCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency: Option<TransparencyStats>,
    pub limits: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    profile: Option<String>,
    pix_fmt: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    nb_frames: Option<String>,
    nb_read_packets: Option<String>,
    duration: Option<String>,
    color_space: Option<String>,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    color_range: Option<String>,
}

pub fn parse_rational(value: &str) -> Option<(u64, u64)> {
    let (num, den) = value.split_once('/')?;
    let (num, den) = (
        num.trim().parse::<u64>().ok()?,
        den.trim().parse::<u64>().ok()?,
    );
    (den != 0).then_some((num, den))
}

fn known(value: Option<String>) -> Option<String> {
    value.filter(|v| {
        let v = v.trim().to_ascii_lowercase();
        !v.is_empty() && v != "unknown" && v != "unspecified" && v != "n/a"
    })
}

pub fn probe(path: &Path) -> Result<ProbeFacts> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-count_packets", "-show_streams", "-show_format"])
        .args(["-print_format", "json"])
        .arg(path)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow!("ffprobe was not found on PATH. Install ffmpeg/ffprobe to verify encoded output.")
            } else {
                anyhow!("failed to spawn ffprobe for {}: {error}", path.display())
            }
        })?;
    if !output.status.success() {
        bail!(
            "ffprobe could not read {} (exit {}): {}",
            path.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse ffprobe JSON for {}", path.display()))?;
    let stream = parsed
        .streams
        .into_iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| {
            anyhow!(
                "ffprobe did not report a video stream in {}",
                path.display()
            )
        })?;
    let pix_fmt = stream.pix_fmt.clone();
    let format = parsed.format;
    Ok(ProbeFacts {
        container: format
            .as_ref()
            .and_then(|f| f.format_name.clone())
            .unwrap_or_default(),
        codec: stream.codec_name,
        profile: stream.profile,
        alpha_capable_pix_fmt: pix_fmt
            .as_deref()
            .is_some_and(|p| p.to_ascii_lowercase().starts_with("yuva")),
        pix_fmt,
        width: stream.width,
        height: stream.height,
        r_frame_rate: stream.r_frame_rate,
        avg_frame_rate: stream.avg_frame_rate,
        advertised_nb_frames: stream.nb_frames.as_deref().and_then(|v| v.parse().ok()),
        packet_count: stream
            .nb_read_packets
            .as_deref()
            .and_then(|v| v.parse().ok()),
        stream_duration_seconds: stream.duration.as_deref().and_then(|v| v.parse().ok()),
        format_duration_seconds: format
            .as_ref()
            .and_then(|f| f.duration.as_deref())
            .and_then(|v| v.parse().ok()),
        color_space: known(stream.color_space),
        color_primaries: known(stream.color_primaries),
        color_transfer: known(stream.color_transfer),
        color_range: known(stream.color_range),
    })
}

/// Decode to RGBA and measure alpha. `max_frames`: decode at most this many frames evenly spread
/// via an ffmpeg `select` filter; `None` decodes every frame.
pub fn measure_transparency(
    path: &Path,
    width: u32,
    height: u32,
    total_frames: u64,
    max_frames: Option<u64>,
) -> Result<TransparencyStats> {
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-i"]).arg(path);
    let mode;
    match max_frames {
        Some(limit) if total_frames > limit && limit > 0 => {
            let picks: Vec<u64> = (0..limit)
                .map(|i| i * (total_frames - 1) / (limit - 1).max(1))
                .collect();
            let expr = picks
                .iter()
                .map(|n| format!("eq(n\\,{n})"))
                .collect::<Vec<_>>()
                .join("+");
            command.args([
                "-vf",
                &format!("select='{expr}'"),
                "-fps_mode",
                "passthrough",
            ]);
            mode = format!("sampled {limit} of {total_frames} frames");
        }
        _ => mode = "all frames".to_owned(),
    }
    command
        .args(["-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| anyhow!("failed to spawn ffmpeg for decode check: {e}"))?;
    let mut stdout = child.stdout.take().context("ffmpeg stdout unavailable")?;
    let frame_bytes = width as usize * height as usize * 4;
    let mut buffer = vec![0u8; frame_bytes];
    let mut stats = TransparencyStats {
        frames_decoded: 0,
        frames_with_transparency: 0,
        min_alpha: 255,
        transparent_pixel_fraction_max: 0.0,
        fully_transparent_pixel_fraction_max: 0.0,
        first_frame_min_alpha: None,
        last_frame_min_alpha: None,
        mode,
    };
    let pixels = (width as f64) * (height as f64);
    loop {
        match stdout.read_exact(&mut buffer) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => bail!("failed reading decoded frames: {error}"),
        }
        let (mut min_alpha, mut partial, mut zero) = (255u8, 0u64, 0u64);
        for pixel in buffer.chunks_exact(4) {
            let alpha = pixel[3];
            if alpha < 255 {
                partial += 1;
                if alpha == 0 {
                    zero += 1;
                }
            }
            min_alpha = min_alpha.min(alpha);
        }
        stats.frames_decoded += 1;
        if partial > 0 {
            stats.frames_with_transparency += 1;
        }
        stats.min_alpha = stats.min_alpha.min(min_alpha);
        stats.transparent_pixel_fraction_max = stats
            .transparent_pixel_fraction_max
            .max(partial as f64 / pixels);
        stats.fully_transparent_pixel_fraction_max = stats
            .fully_transparent_pixel_fraction_max
            .max(zero as f64 / pixels);
        if stats.first_frame_min_alpha.is_none() {
            stats.first_frame_min_alpha = Some(min_alpha);
        }
        stats.last_frame_min_alpha = Some(min_alpha);
    }
    let status = child.wait().context("failed waiting for ffmpeg decode")?;
    if !status.success() && stats.frames_decoded == 0 {
        let mut stderr = String::new();
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_string(&mut stderr);
        }
        bail!(
            "ffmpeg could not decode {}: {}",
            path.display(),
            stderr.trim()
        );
    }
    Ok(stats)
}

fn check(
    id: &'static str,
    expected: Option<Value>,
    observed: Option<Value>,
    pass: Option<bool>,
    note: Option<String>,
) -> MediaCheck {
    MediaCheck {
        id,
        status: match pass {
            Some(true) => CheckStatus::Pass,
            Some(false) => CheckStatus::Fail,
            None => CheckStatus::Skipped,
        },
        expected,
        observed,
        note,
    }
}

fn profile_token(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Probe `path` and evaluate `expected`. `decode_frame_budget`: cap on frames decoded for the
/// transparency measurement (`None` = all).
pub fn verify_media(
    path: &Path,
    expected: &Expectations,
    decode_frame_budget: Option<u64>,
) -> Result<MediaReport> {
    let probe = probe(path)?;
    let mut checks = Vec::new();
    let mut limits = vec![
        "frame count is the demuxed packet count (exact for intra-only codecs like ProRes)".to_owned(),
        "frame rate compared as exact rationals; duration compared within half a frame period".to_owned(),
        "transparency measured on decoded 8-bit RGBA; alpha below 8-bit resolution is not distinguished".to_owned(),
        "does not judge visual quality; see `vcr inspect` for motion/bounds diagnostics".to_owned(),
    ];

    // Container: ffprobe reports "mov,mp4,m4a,3gp,3g2,mj2" for QuickTime-family files.
    let container_ok = expected.container.as_ref().map(|want| {
        probe
            .container
            .split(',')
            .any(|name| name.eq_ignore_ascii_case(want))
    });
    checks.push(check(
        "container",
        expected.container.as_ref().map(|c| json!(c)),
        Some(json!(probe.container)),
        container_ok,
        None,
    ));

    checks.push(check(
        "codec",
        expected.codec.as_ref().map(|c| json!(c)),
        probe.codec.as_ref().map(|c| json!(c)),
        expected.codec.as_ref().map(|want| {
            probe
                .codec
                .as_deref()
                .is_some_and(|got| got.eq_ignore_ascii_case(want))
        }),
        None,
    ));
    checks.push(check(
        "profile",
        expected.profile.as_ref().map(|c| json!(c)),
        probe.profile.as_ref().map(|c| json!(c)),
        expected.profile.as_ref().map(|want| {
            probe
                .profile
                .as_deref()
                .is_some_and(|got| profile_token(got) == profile_token(want))
        }),
        None,
    ));
    checks.push(check(
        "resolution",
        match (expected.width, expected.height) {
            (Some(w), Some(h)) => Some(json!({"width": w, "height": h})),
            _ => None,
        },
        Some(json!({"width": probe.width, "height": probe.height})),
        match (expected.width, expected.height) {
            (Some(w), Some(h)) => Some(probe.width == Some(w) && probe.height == Some(h)),
            _ => None,
        },
        None,
    ));

    let rate = probe.r_frame_rate.as_deref().and_then(parse_rational);
    checks.push(check(
        "frame_rate",
        expected.fps.map(|f| json!(format!("{f}/1"))),
        probe.r_frame_rate.as_ref().map(|r| json!(r)),
        expected
            .fps
            .map(|want| rate.is_some_and(|(num, den)| num == u64::from(want) * den)),
        Some("exact rational comparison".to_owned()),
    ));

    let frames = probe.packet_count;
    checks.push(check(
        "frame_count",
        expected.frame_count.map(|f| json!(f)),
        frames.map(|f| json!(f)),
        expected
            .frame_count
            .map(|want| frames == Some(u64::from(want))),
        probe
            .advertised_nb_frames
            .filter(|adv| Some(*adv) != frames)
            .map(|adv| {
                format!(
                    "container advertises nb_frames={adv}, which disagrees with the packet count"
                )
            }),
    ));

    // Duration: expected exact rational frames/fps; observed stream/format duration.
    let duration_expect = expected
        .frame_count
        .zip(expected.fps)
        .map(|(frames, fps)| frames as f64 / fps as f64);
    let observed_duration = probe
        .stream_duration_seconds
        .or(probe.format_duration_seconds);
    let duration_ok = duration_expect.zip(expected.fps).map(|(want, fps)| {
        observed_duration.is_some_and(|got| (got - want).abs() <= 0.5 / fps as f64 + 1e-9)
    });
    checks.push(check(
        "duration_seconds",
        duration_expect.map(|d| json!(d)),
        observed_duration.map(|d| json!(d)),
        duration_ok,
        Some("tolerance: half a frame period".to_owned()),
    ));

    checks.push(check(
        "alpha_capable",
        expected.alpha_capable.map(|a| json!(a)),
        Some(json!(probe.alpha_capable_pix_fmt)),
        expected
            .alpha_capable
            .map(|want| probe.alpha_capable_pix_fmt == want),
        probe.pix_fmt.as_ref().map(|p| format!("pix_fmt={p}")),
    ));

    // Decoded transparency.
    let mut transparency = None;
    let wants_decode =
        expected.transparency != TransparencyExpectation::Any || probe.alpha_capable_pix_fmt;
    if wants_decode {
        if let (Some(w), Some(h)) = (probe.width, probe.height) {
            let total = probe.packet_count.unwrap_or(0);
            match measure_transparency(path, w, h, total, decode_frame_budget) {
                Ok(stats) => {
                    if stats.mode != "all frames" {
                        limits.push(format!(
                            "transparency {}; unsampled frames not examined",
                            stats.mode
                        ));
                    }
                    transparency = Some(stats);
                }
                Err(error) => checks.push(check(
                    "decode",
                    None,
                    Some(json!(error.to_string())),
                    Some(false),
                    Some("file could not be decoded".to_owned()),
                )),
            }
        }
    }
    let has_transparency = transparency
        .as_ref()
        .map(|s| s.frames_with_transparency > 0);
    checks.push(check(
        "transparency",
        match expected.transparency {
            TransparencyExpectation::Required => Some(json!("some decoded pixel with alpha < 255")),
            TransparencyExpectation::None => Some(json!("all decoded pixels opaque")),
            TransparencyExpectation::Any => None,
        },
        transparency.as_ref().map(|s| {
            json!({
                "frames_with_transparency": s.frames_with_transparency,
                "frames_decoded": s.frames_decoded,
                "min_alpha": s.min_alpha,
                "fully_transparent_pixel_fraction_max": s.fully_transparent_pixel_fraction_max,
            })
        }),
        match (expected.transparency, has_transparency) {
            (TransparencyExpectation::Required, Some(has)) => Some(has),
            (TransparencyExpectation::None, Some(has)) => Some(!has),
            (TransparencyExpectation::Any, _) => None,
            (_, None) => Some(false),
        },
        Some(match expected.transparency {
            TransparencyExpectation::Any => "no expectation; observation reported only".to_owned(),
            _ => "measured on decoded pixels, not inferred from pix_fmt".to_owned(),
        }),
    ));

    let passed = !checks.iter().any(|c| c.status == CheckStatus::Fail);
    Ok(MediaReport {
        passed,
        probe,
        checks,
        transparency,
        limits,
    })
}
