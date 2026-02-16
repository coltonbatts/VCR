// Embed git hash for --version. Optional; no git = no hash.
fn main() {
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        if output.status.success() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let hash = s.trim();
                println!("cargo:rustc-env=VCR_GIT_HASH={hash}");
            }
        }
    }
}
