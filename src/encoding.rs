use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, bail, Context, Result};

use crate::schema::Environment;

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

    let mut child = Command::new("ffmpeg")
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
        .arg("4444")
        .arg("-pix_fmt")
        .arg("yuva444p10le")
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
