use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::workflow::types::RenderOutput;

const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");

pub fn check_manifest(manifest_path: &Path, timeout: Duration) -> Result<()> {
    let args = vec![
        "run".to_owned(),
        "--release".to_owned(),
        "--bin".to_owned(),
        "vcr".to_owned(),
        "--".to_owned(),
        "check".to_owned(),
        manifest_path.display().to_string(),
    ];
    let output = run_cargo_command(args, timeout)?;
    if !output.status.success() {
        bail!(
            "VCR manifest check failed.\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );
    }
    Ok(())
}

pub fn render_manifest(
    manifest_path: &Path,
    output_path: &Path,
    timeout: Duration,
) -> Result<RenderOutput> {
    let args = vec![
        "run".to_owned(),
        "--release".to_owned(),
        "--bin".to_owned(),
        "vcr".to_owned(),
        "--".to_owned(),
        "build".to_owned(),
        manifest_path.display().to_string(),
        "-o".to_owned(),
        output_path.display().to_string(),
    ];
    let started = Instant::now();
    let output = run_cargo_command(args, timeout)?;
    if !output.status.success() {
        bail!(
            "VCR render failed.\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );
    }
    if !output_path.exists() {
        bail!(
            "VCR render exited successfully, but output file was not found: {}",
            output_path.display()
        );
    }

    Ok(RenderOutput {
        output_path: output_path.to_path_buf(),
        stdout: output.stdout,
        stderr: output.stderr,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_cargo_command(args: Vec<String>, timeout: Duration) -> Result<CommandOutput> {
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(PathBuf::from(REPO_ROOT))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .context("failed to launch cargo command for VCR integration")?;
    let started = Instant::now();

    loop {
        if child
            .try_wait()
            .context("failed while waiting for cargo command")?
            .is_some()
        {
            break;
        }

        if started.elapsed() > timeout {
            child
                .kill()
                .context("failed to kill timed-out VCR process")?;
            let _ = child.wait();
            bail!("VCR command timed out after {} seconds", timeout.as_secs());
        }

        thread::sleep(Duration::from_millis(100));
    }

    let output = child
        .wait_with_output()
        .context("failed to collect VCR command output")?;
    Ok(CommandOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}
