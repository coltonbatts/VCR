use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, Context, Result};

pub struct FfmpegInput {
    receiver: mpsc::Receiver<Vec<u8>>,
    worker: Option<JoinHandle<Result<()>>>,
    child: Child,
}

impl FfmpegInput {
    pub fn spawn(input_path: &Path, width: u32, height: u32) -> Result<Self> {
        let size = format!("{}x{}", width, height);
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(4);
        let input_path = input_path.to_path_buf();

        let mut child = Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-i")
            .arg(&input_path)
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-s")
            .arg(size)
            .arg("-sws_flags")
            .arg("area")
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to spawn ffmpeg decoder")?;

        let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("failed to capture ffmpeg stdout"))?;
        let frame_size = (width * height * 4) as usize;

        let worker = thread::Builder::new()
            .name("vcr-ffmpeg-decoder".to_owned())
            .spawn(move || {
                loop {
                    let mut buffer = vec![0u8; frame_size];
                    match stdout.read_exact(&mut buffer) {
                        Ok(_) => {
                            if sender.send(buffer).is_err() {
                                break;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                        Err(e) => return Err(anyhow!("failed to read from ffmpeg: {e}")),
                    }
                }
                Ok(())
            })
            .context("failed to spawn ffmpeg reader thread")?;

        Ok(Self {
            receiver,
            worker: Some(worker),
            child,
        })
    }

    pub fn read_frame(&self) -> Option<Vec<u8>> {
        self.receiver.recv().ok()
    }

    pub fn finish(mut self) -> Result<()> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        
        if let Some(handle) = self.worker.take() {
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(anyhow!("ffmpeg reader thread panicked")),
            }
        } else {
            Ok(())
        }
    }
}
