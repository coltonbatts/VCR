use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, bail, Context, Result};

use crate::schema::Environment;

/// Makes the RGBA -> Y'CbCr step of an FFmpeg encode explicit and tagged.
///
/// VCR frames are 8-bit straight-alpha RGBA whose RGB is sRGB-encoded with BT.709/sRGB
/// primaries (docs/COLOR_PIPELINE.md). Without these arguments FFmpeg converts with its
/// BT.601 default matrix and writes no color tags, so BT.709-assuming players shift colors.
///
/// - matrix: BT.709, full-range RGB into limited ("tv") range Y'CbCr
/// - tags: primaries, transfer, matrix = BT.709; range = tv
/// - transfer: sRGB-encoded values pass through unchanged and are tagged BT.709, the
///   convention for display-referred graphics delivered as HD video (the curves differ only
///   in the deep shadows; VCR does not re-encode)
/// - swscale rounding is accurate and bit-exact so encodes are reproducible across machines
///
/// Tags are set both on the frames (`setparams`, which newer FFmpeg encoders read) and as
/// output options (which older FFmpeg versions read).
pub fn push_bt709_encode_args(command: &mut Command, pix_fmt: &str) {
    command
        .arg("-vf")
        .arg(format!(
            "scale=out_color_matrix=bt709:out_range=tv:flags=accurate_rnd+full_chroma_int+bitexact,\
             format={pix_fmt},\
             setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv"
        ))
        .arg("-pix_fmt")
        .arg(pix_fmt)
        .arg("-color_primaries")
        .arg("bt709")
        .arg("-color_trc")
        .arg("bt709")
        .arg("-colorspace")
        .arg("bt709")
        .arg("-color_range")
        .arg("tv");
}

pub struct FfmpegPipe {
    sender: Option<mpsc::SyncSender<Vec<u8>>>,
    worker: Option<JoinHandle<Result<()>>>,
}

impl FfmpegPipe {
    pub fn spawn(environment: &Environment, output_path: &Path) -> Result<Self> {
        let size = format!(
            "{}x{}",
            environment.resolution.width, environment.resolution.height
        );
        let fps = environment.fps.to_string();
        let output_path = output_path.to_path_buf();
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(4);

        let worker = thread::Builder::new()
            .name("vcr-ffmpeg-encoder".to_owned())
            .spawn(move || encoding_worker(receiver, size, fps, &output_path))
            .context("failed to spawn ffmpeg writer thread")?;

        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    pub fn write_frame(&self, rgba_frame: Vec<u8>) -> Result<()> {
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| anyhow!("encoder has already been finalized"))?;
        sender
            .send(rgba_frame)
            .map_err(|_| anyhow!("failed to enqueue frame for ffmpeg"))
    }

    pub fn finish(mut self) -> Result<()> {
        drop(self.sender.take());

        let handle = self
            .worker
            .take()
            .ok_or_else(|| anyhow!("ffmpeg worker thread missing"))?;
        match handle.join() {
            Ok(result) => result,
            Err(_) => Err(anyhow!("ffmpeg worker thread panicked")),
        }
    }
}

fn encoding_worker(
    receiver: mpsc::Receiver<Vec<u8>>,
    size: String,
    fps: String,
    output_path: &Path,
) -> Result<()> {
    // Basic sanity check on output path
    let path_str = output_path.to_string_lossy();
    if path_str.len() > 1024 {
        bail!("Output path is suspiciously long");
    }
    if path_str.chars().any(|c| c.is_control()) {
        bail!("Output path contains invalid control characters");
    }

    let mut command = Command::new("ffmpeg");
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("-s:v")
        .arg(size)
        .arg("-r")
        .arg(fps)
        .arg("-i")
        .arg("-")
        .arg("-an")
        .arg("-c:v")
        .arg("prores_ks")
        .arg("-profile:v")
        .arg("4444");
    push_bt709_encode_args(&mut command, "yuva444p10le");
    let mut child = command
        .arg(output_path.as_os_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                anyhow!(
                    "ffmpeg was not found on PATH. Install ffmpeg and verify `ffmpeg -version` works before running `vcr build` or `vcr preview` video output."
                )
            } else {
                anyhow!("failed to spawn ffmpeg sidecar process: {error}")
            }
        })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to capture ffmpeg stdin"))?;

    while let Ok(frame) = receiver.recv() {
        stdin
            .write_all(&frame)
            .context("failed to write frame to ffmpeg stdin")?;
    }

    stdin.flush().context("failed to flush ffmpeg stdin")?;
    drop(stdin);

    let status = child.wait().context("failed waiting for ffmpeg process")?;
    if !status.success() {
        return Err(anyhow!("ffmpeg failed with status {status}"));
    }

    Ok(())
}
