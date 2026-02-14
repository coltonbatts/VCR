use std::io::{ErrorKind, Read, Write};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegMode {
    Auto,
    System,
    Sidecar,
}

trait VideoEncoderBackend: Send {
    fn mode_label(&self) -> &'static str;
    fn run(self: Box<Self>, receiver: mpsc::Receiver<Vec<u8>>) -> Result<()>;
}

struct SystemFfmpegBackend {
    size: String,
    fps: String,
    encoding: crate::schema::EncodingConfig,
    output_path: std::path::PathBuf,
}

#[cfg(feature = "sidecar_ffmpeg")]
struct SidecarFfmpegBackend {
    size: String,
    fps: String,
    encoding: crate::schema::EncodingConfig,
    output_path: std::path::PathBuf,
}

impl FfmpegPipe {
    pub fn spawn(environment: &Environment, output_path: &Path) -> Result<Self> {
        Self::spawn_with_mode(environment, output_path, FfmpegMode::Auto)
    }

    pub fn spawn_with_mode(
        environment: &Environment,
        output_path: &Path,
        mode: FfmpegMode,
    ) -> Result<Self> {
        let size = format!(
            "{}x{}",
            environment.resolution.width, environment.resolution.height
        );
        let fps = environment.fps.to_string();
        let encoding = environment.encoding.clone();
        let output_path = output_path.to_path_buf();
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(4);
        let backend = select_backend(mode, size, fps, encoding, output_path)?;
        let worker_name = format!("vcr-ffmpeg-encoder-{}", backend.mode_label());

        let worker = thread::Builder::new()
            .name(worker_name)
            .spawn(move || backend.run(receiver))
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

fn select_backend(
    mode: FfmpegMode,
    size: String,
    fps: String,
    encoding: crate::schema::EncodingConfig,
    output_path: std::path::PathBuf,
) -> Result<Box<dyn VideoEncoderBackend>> {
    match mode {
        FfmpegMode::Auto | FfmpegMode::System => Ok(Box::new(SystemFfmpegBackend {
            size,
            fps,
            encoding,
            output_path,
        })),
        FfmpegMode::Sidecar => {
            #[cfg(feature = "sidecar_ffmpeg")]
            {
                Ok(Box::new(SidecarFfmpegBackend {
                    size,
                    fps,
                    encoding,
                    output_path,
                }))
            }
            #[cfg(not(feature = "sidecar_ffmpeg"))]
            {
                Err(anyhow!(
                    "ffmpeg sidecar mode requested but VCR was built without `sidecar_ffmpeg`. Rebuild with `--features sidecar_ffmpeg`."
                ))
            }
        }
    }
}

impl VideoEncoderBackend for SystemFfmpegBackend {
    fn mode_label(&self) -> &'static str {
        "system"
    }

    fn run(self: Box<Self>, receiver: mpsc::Receiver<Vec<u8>>) -> Result<()> {
        run_ffmpeg_process(
            Path::new("ffmpeg"),
            receiver,
            &self.size,
            &self.fps,
            &self.encoding,
            &self.output_path,
            self.mode_label(),
        )
    }
}

#[cfg(feature = "sidecar_ffmpeg")]
impl VideoEncoderBackend for SidecarFfmpegBackend {
    fn mode_label(&self) -> &'static str {
        "sidecar"
    }

    fn run(self: Box<Self>, receiver: mpsc::Receiver<Vec<u8>>) -> Result<()> {
        let path = ffmpeg_sidecar::paths::ffmpeg_path();
        if !path.exists() {
            ffmpeg_sidecar::download::auto_download()
                .context("failed to auto-download ffmpeg sidecar binary")?;
        }
        run_ffmpeg_process(
            &path,
            receiver,
            &self.size,
            &self.fps,
            &self.encoding,
            &self.output_path,
            self.mode_label(),
        )
    }
}

fn run_ffmpeg_process(
    ffmpeg_path: &Path,
    receiver: mpsc::Receiver<Vec<u8>>,
    size: &str,
    fps: &str,
    encoding: &crate::schema::EncodingConfig,
    output_path: &Path,
    mode_label: &str,
) -> Result<()> {
    // Basic sanity check on output path
    let path_str = output_path.to_string_lossy();
    if path_str.len() > 1024 {
        bail!("Output path is suspiciously long");
    }
    if path_str.chars().any(|c| c.is_control()) {
        bail!("Output path contains invalid control characters");
    }

    let args = ffmpeg_args(size, fps, encoding, output_path);
    let mut command = Command::new(ffmpeg_path);
    command
        .args(args.iter().map(String::as_str))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                anyhow!(
                    "ffmpeg executable not found (mode={mode_label}, resolved_path={}). Install ffmpeg (system mode) or use sidecar mode with `--features sidecar_ffmpeg`.",
                    ffmpeg_path.display()
                )
            } else {
                anyhow!(
                    "failed to spawn ffmpeg process (mode={mode_label}, resolved_path={}, args='{}'): {error}",
                    ffmpeg_path.display(),
                    args.join(" ")
                )
            }
        })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to capture ffmpeg stdin"))?;
    let mut stderr_pipe = child.stderr.take();

    while let Ok(frame) = receiver.recv() {
        stdin
            .write_all(&frame)
            .context("failed to write frame to ffmpeg stdin")?;
    }

    stdin.flush().context("failed to flush ffmpeg stdin")?;
    drop(stdin);

    let status = child.wait().context("failed waiting for ffmpeg process")?;
    let stderr_tail = read_stderr_tail(&mut stderr_pipe)?;
    if !status.success() {
        return Err(anyhow!(
            "ffmpeg failed with status {status} (mode={mode_label}, resolved_path={}, args='{}', stderr_tail='{}')",
            ffmpeg_path.display(),
            args.join(" "),
            stderr_tail
        ));
    }

    Ok(())
}

fn ffmpeg_args(
    size: &str,
    fps: &str,
    encoding: &crate::schema::EncodingConfig,
    output_path: &Path,
) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
        "-y".to_owned(),
        "-f".to_owned(),
        "rawvideo".to_owned(),
        "-pix_fmt".to_owned(),
        "rgba".to_owned(),
        "-s:v".to_owned(),
        size.to_owned(),
        "-r".to_owned(),
        fps.to_owned(),
        "-i".to_owned(),
        "-".to_owned(),
        "-an".to_owned(),
        "-c:v".to_owned(),
        "prores_ks".to_owned(),
        "-profile:v".to_owned(),
        encoding.prores_profile.to_ffmpeg_profile().to_owned(),
        "-vendor".to_owned(),
        encoding.vendor.clone(),
    ];

    if encoding.prores_profile == crate::schema::ProResProfile::Prores4444
        || encoding.prores_profile == crate::schema::ProResProfile::Prores4444Xq
    {
        args.push("-pix_fmt".to_owned());
        args.push("yuva444p10le".to_owned());
    } else {
        args.push("-pix_fmt".to_owned());
        args.push("yuv422p10le".to_owned());
    }

    args.push(output_path.to_string_lossy().into_owned());
    args
}

fn read_stderr_tail(stderr: &mut Option<std::process::ChildStderr>) -> Result<String> {
    let Some(mut pipe) = stderr.take() else {
        return Ok(String::new());
    };
    let mut buf = Vec::new();
    pipe.read_to_end(&mut buf)
        .context("failed reading ffmpeg stderr")?;
    let text = String::from_utf8_lossy(&buf).to_string();
    Ok(last_n_chars(&text, 500))
}

fn last_n_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars().collect::<Vec<_>>();
    if chars.len() > max_chars {
        chars = chars[chars.len().saturating_sub(max_chars)..].to_vec();
    }
    chars.into_iter().collect::<String>().trim().to_owned()
}
