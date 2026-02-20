use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

const DECK_BINARY: &str = "tape-deck";
const DECK_MODULE_DIR: &str = "vhs-tape-deck";
const DECK_BUILD_PKG: &str = "./cmd/tape-deck";
const SYSTEM_INSTALL_DIR: &str = "/usr/local/bin";

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        return Ok(());
    };

    match command.as_str() {
        "deck-install" => {
            let mut system = false;
            for arg in args {
                match arg.as_str() {
                    "--system" => system = true,
                    "--help" | "-h" => {
                        print_deck_install_help();
                        return Ok(());
                    }
                    other => {
                        return Err(format!(
                            "unknown argument '{other}' for 'deck-install' (try: cargo xtask deck-install --help)"
                        ));
                    }
                }
            }
            deck_install(system)
        }
        "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => Err(format!(
            "unknown xtask command '{other}' (try: cargo xtask --help)"
        )),
    }
}

fn deck_install(system: bool) -> Result<(), String> {
    ensure_go_available()?;

    let repo_root = repo_root()?;
    let deck_dir = repo_root.join(DECK_MODULE_DIR);
    if !deck_dir.is_dir() {
        return Err(format!(
            "deck source directory not found: {}",
            deck_dir.display()
        ));
    }

    let install_dir = if system {
        let path = PathBuf::from(SYSTEM_INSTALL_DIR);
        ensure_writable_dir(&path)?;
        path
    } else {
        default_user_install_dir()?
    };

    if !system {
        fs::create_dir_all(&install_dir).map_err(|error| {
            format!(
                "failed to create install directory {}: {error}",
                install_dir.display()
            )
        })?;
    }

    let build_dir = repo_root.join("target").join("xtask");
    fs::create_dir_all(&build_dir).map_err(|error| {
        format!(
            "failed to create build directory {}: {error}",
            build_dir.display()
        )
    })?;

    let built_binary = build_dir.join(DECK_BINARY);
    let install_path = install_dir.join(DECK_BINARY);

    build_deck_binary(&deck_dir, &built_binary)?;
    fs::copy(&built_binary, &install_path).map_err(|error| {
        format!(
            "failed to install {} to {}: {error}",
            DECK_BINARY,
            install_path.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&install_path)
            .map_err(|error| format!("failed to read installed file metadata: {error}"))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&install_path, perms)
            .map_err(|error| format!("failed to set executable permissions: {error}"))?;
    }

    println!("Installed {DECK_BINARY} to {}", install_path.display());

    if dir_is_on_path(&install_dir) {
        println!(
            "PATH check: {} is on PATH, so exec.LookPath(\"{DECK_BINARY}\") should succeed.",
            install_dir.display()
        );
    } else {
        println!(
            "PATH check: {} is not on PATH, so exec.LookPath(\"{DECK_BINARY}\") would fail.",
            install_dir.display()
        );
        print_path_snippets(&install_dir);
    }

    Ok(())
}

fn repo_root() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().map(Path::to_path_buf).ok_or_else(|| {
        format!(
            "failed to resolve repository root from {}",
            manifest_dir.display()
        )
    })
}

fn ensure_go_available() -> Result<(), String> {
    let status = Command::new("go")
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "Go toolchain check failed ('go version' exited with {:?}). Install Go first: https://go.dev/dl/",
            status.code()
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(
            "Go toolchain not found on PATH. Install Go first: https://go.dev/dl/".to_owned(),
        ),
        Err(error) => Err(format!("failed to run 'go version': {error}")),
    }
}

fn build_deck_binary(deck_dir: &Path, output_binary: &Path) -> Result<(), String> {
    println!("Building {DECK_BINARY} from {}", deck_dir.display());

    let status = Command::new("go")
        .current_dir(deck_dir)
        .arg("build")
        .arg("-o")
        .arg(output_binary)
        .arg(DECK_BUILD_PKG)
        .status()
        .map_err(|error| format!("failed to run go build: {error}"))?;

    if !status.success() {
        return Err(format!(
            "go build failed (exit status {:?}) while building {}",
            status.code(),
            DECK_BUILD_PKG
        ));
    }

    Ok(())
}

fn default_user_install_dir() -> Result<PathBuf, String> {
    let home = home_dir()?;
    Ok(home.join(".local").join("bin"))
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
        .ok_or_else(|| "cannot resolve home directory from HOME/USERPROFILE".to_owned())
}

fn ensure_writable_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!(
            "system install target {} does not exist or is not a directory",
            dir.display()
        ));
    }

    let probe = dir.join(format!(".vcr_xtask_write_probe_{}", process::id()));
    let file = OpenOptions::new().write(true).create_new(true).open(&probe);
    match file {
        Ok(_) => {
            let _ = fs::remove_file(probe);
            Ok(())
        }
        Err(error) => Err(format!(
            "'--system' requested but {} is not writable: {error}",
            dir.display()
        )),
    }
}

fn dir_is_on_path(target_dir: &Path) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&path_var).any(|entry| {
        normalize_for_compare(&entry)
            .as_deref()
            .zip(normalize_for_compare(target_dir).as_deref())
            .map(|(lhs, rhs)| lhs == rhs)
            .unwrap_or(false)
    })
}

fn normalize_for_compare(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().ok()?.join(path)
    };
    Some(absolute)
}

fn path_expr_for_shell(path: &Path) -> String {
    if let Ok(home) = home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "$HOME".to_owned();
            }
            return format!("$HOME/{}", rest.display());
        }
    }
    path.display().to_string()
}

fn print_path_snippets(install_dir: &Path) {
    let path_expr = path_expr_for_shell(install_dir);
    println!("Add this directory to PATH:");
    println!(
        "  zsh : echo 'export PATH=\"{}:$PATH\"' >> ~/.zshrc",
        path_expr
    );
    println!(
        "  bash: echo 'export PATH=\"{}:$PATH\"' >> ~/.bashrc",
        path_expr
    );
    println!("  fish: fish_add_path {}", path_expr);
}

fn print_usage() {
    println!("Usage:");
    println!("  cargo xtask deck-install [--system]");
}

fn print_deck_install_help() {
    println!("Install the first-party tape deck binary from this repository.");
    println!();
    print_usage();
    println!();
    println!("Options:");
    println!("  --system   Install to /usr/local/bin/tape-deck (requires writable /usr/local/bin)");
    println!("             Default install target is ~/.local/bin/tape-deck");
}
