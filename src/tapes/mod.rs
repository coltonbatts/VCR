use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Local};
use regex::Regex;
use serde::{Deserialize, Serialize};

const DEFAULT_CONFIG_DIR_NAME: &str = "vcr";
const DEFAULT_CONFIG_FILE_NAME: &str = "tapes.yaml";
const DEFAULT_VCR_BINARY: &str = "vcr";
const DEFAULT_OUTPUT_FLAG: &str = "--output";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TapeMode {
    Video,
    Frame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TapeAction {
    Primary,
    Preview,
}

impl TapeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            TapeAction::Primary => "primary",
            TapeAction::Preview => "preview",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapeAesthetic {
    #[serde(default)]
    pub label_style: Option<String>,
    #[serde(default)]
    pub shell_colorway: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapePreviewFile {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub frame: Option<i64>,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Default for TapePreviewFile {
    fn default() -> Self {
        Self {
            enabled: false,
            frame: Some(0),
            args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapeFile {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub manifest: String,
    pub mode: TapeMode,
    #[serde(default)]
    pub primary_args: Vec<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub preview: TapePreviewFile,
    #[serde(default)]
    pub aesthetic: Option<TapeAesthetic>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TapeStoreFile {
    #[serde(default)]
    pub vcr_binary: Option<String>,
    #[serde(default)]
    pub output_flag: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub runs_dir: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub tapes: Vec<TapeFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapePreview {
    pub enabled: bool,
    pub frame: u32,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tape {
    pub id: String,
    pub name: String,
    pub manifest: String,
    pub resolved_manifest_path: PathBuf,
    pub mode: TapeMode,
    pub primary_args: Vec<String>,
    pub output_dir: PathBuf,
    pub preview: TapePreview,
    pub aesthetic: Option<TapeAesthetic>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapeStore {
    pub config_path: PathBuf,
    pub config_dir: PathBuf,
    pub vcr_binary: String,
    pub output_flag: String,
    pub project_root: PathBuf,
    pub runs_dir: PathBuf,
    pub env: BTreeMap<String, String>,
    pub tapes: Vec<Tape>,
}

impl TapeStore {
    pub fn tape_by_id(&self, tape_id: &str) -> Option<&Tape> {
        self.tapes.iter().find(|tape| tape.id == tape_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapeCommandPlan {
    pub timestamp: DateTime<Local>,
    pub run_id: String,
    pub binary: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env_overrides: BTreeMap<String, String>,
    pub manifest_path: PathBuf,
    pub output_paths: Vec<PathBuf>,
    pub action: TapeAction,
    pub dry_run: bool,
    pub record_path: PathBuf,
    pub explicit_subcommand: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub timestamp: String,
    pub run_id: String,
    pub tape_id: String,
    pub tape_name: String,
    pub resolved_manifest_path: String,
    pub resolved_command: Vec<String>,
    pub cwd: String,
    pub env_overrides: BTreeMap<String, String>,
    pub exit_code: i32,
    pub output_paths: Vec<String>,
    pub action: String,
    pub dry_run: bool,
    pub record_path_written: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub record: RunRecord,
    pub record_path: PathBuf,
    pub first_output_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureProbe {
    pub checked: bool,
    pub has_render_frame: bool,
    pub help_snippet: String,
    pub detection_failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub ok: bool,
    pub diagnostics: Vec<String>,
}

static RUN_ID_COUNTERS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
static FEATURE_CACHE: OnceLock<Mutex<HashMap<String, FeatureProbe>>> = OnceLock::new();

pub fn default_config_path() -> Result<PathBuf> {
    let base = user_config_dir()?;
    Ok(base
        .join(DEFAULT_CONFIG_DIR_NAME)
        .join(DEFAULT_CONFIG_FILE_NAME))
}

pub fn resolve_config_path(config_override: Option<&Path>, launch_cwd: &Path) -> Result<PathBuf> {
    let raw = if let Some(path) = config_override {
        path.to_path_buf()
    } else {
        default_config_path()?
    };
    resolve_against_base_path(&raw, launch_cwd)
}

pub fn load_store(config_path: &Path, launch_cwd: &Path) -> Result<TapeStore> {
    let file = load_store_file(config_path)?;
    resolve_store(file, config_path, launch_cwd)
}

pub fn load_store_file(config_path: &Path) -> Result<TapeStoreFile> {
    let config_text = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read tape config at {}", config_path.display()))?;
    if config_text.trim().is_empty() {
        return Ok(TapeStoreFile::default());
    }
    let parsed: TapeStoreFile = serde_yaml::from_str(&config_text).with_context(|| {
        format!(
            "failed to parse tape config yaml at {}",
            config_path.display()
        )
    })?;
    Ok(parsed)
}

pub fn save_store_file(config_path: &Path, file: &TapeStoreFile) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    let yaml = serde_yaml::to_string(file).context("failed to serialize tape config yaml")?;
    fs::write(config_path, yaml)
        .with_context(|| format!("failed to write tape config {}", config_path.display()))?;
    Ok(())
}

pub fn resolve_store(
    file: TapeStoreFile,
    config_path: &Path,
    launch_cwd: &Path,
) -> Result<TapeStore> {
    let config_path = resolve_against_base_path(config_path, launch_cwd)?;
    let config_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let vcr_binary = file
        .vcr_binary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_VCR_BINARY)
        .to_owned();

    let output_flag = file
        .output_flag
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_OUTPUT_FLAG)
        .to_owned();
    if !output_flag.starts_with('-') {
        bail!("output_flag must start with '-': {output_flag}");
    }

    let project_root = match file.project_root.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => {
            resolve_string_path(value, &config_dir).context("failed to resolve project_root")?
        }
        _ => resolve_against_base_path(launch_cwd, &config_dir)
            .context("failed to resolve default project_root")?,
    };

    let runs_dir = match file.runs_dir.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => {
            resolve_string_path(value, &config_dir).context("failed to resolve runs_dir")?
        }
        _ => config_dir.join("runs"),
    };
    let runs_dir = normalize_path(runs_dir);

    let mut tapes = Vec::with_capacity(file.tapes.len());
    let mut seen_ids = HashMap::<String, usize>::new();
    for (index, tape_file) in file.tapes.into_iter().enumerate() {
        let tape_id = tape_file.id.trim().to_owned();
        if tape_id.is_empty() {
            bail!("tapes[{index}] has an empty id");
        }
        if let Some(previous_index) = seen_ids.insert(tape_id.clone(), index) {
            bail!(
                "duplicate tape id '{}' found at tapes[{previous_index}] and tapes[{index}]",
                tape_id
            );
        }

        let manifest_raw = tape_file.manifest.trim().to_owned();
        if manifest_raw.is_empty() {
            bail!("tape '{tape_id}' has an empty manifest path");
        }
        let resolved_manifest_path = resolve_string_path(&manifest_raw, &project_root)
            .with_context(|| format!("failed to resolve manifest path for tape '{tape_id}'"))?;

        let name = tape_file
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(tape_id.as_str())
            .to_owned();

        let output_dir = match tape_file.output_dir.as_deref().map(str::trim) {
            Some(value) if !value.is_empty() => resolve_string_path(value, &project_root)
                .with_context(|| format!("failed to resolve output_dir for tape '{tape_id}'"))?,
            _ => normalize_path(runs_dir.join(&tape_id)),
        };

        let preview_frame = match tape_file.preview.frame {
            Some(frame) if frame < 0 => {
                bail!("tape '{tape_id}' has a negative preview frame: {frame}")
            }
            Some(frame) => frame as u32,
            None => 0,
        };

        tapes.push(Tape {
            id: tape_id,
            name,
            manifest: manifest_raw,
            resolved_manifest_path,
            mode: tape_file.mode,
            primary_args: tape_file.primary_args,
            output_dir,
            preview: TapePreview {
                enabled: tape_file.preview.enabled,
                frame: preview_frame,
                args: tape_file.preview.args,
            },
            aesthetic: tape_file.aesthetic,
            notes: tape_file.notes,
        });
    }

    Ok(TapeStore {
        config_path,
        config_dir,
        vcr_binary,
        output_flag,
        project_root,
        runs_dir,
        env: file.env,
        tapes,
    })
}

pub fn starter_store_file(launch_cwd: &Path) -> TapeStoreFile {
    let project_root = launch_cwd.to_string_lossy().to_string();
    TapeStoreFile {
        vcr_binary: Some(DEFAULT_VCR_BINARY.to_owned()),
        output_flag: Some(DEFAULT_OUTPUT_FLAG.to_owned()),
        project_root: Some(project_root),
        runs_dir: None,
        env: BTreeMap::from([(String::from("VCR_SEED"), String::from("0"))]),
        tapes: vec![
            TapeFile {
                id: String::from("alpha-lower-third"),
                name: Some(String::from("Alpha Lower Third")),
                manifest: String::from("./manifests/colton_batts_lower_third.vcr"),
                mode: TapeMode::Video,
                primary_args: Vec::new(),
                output_dir: Some(String::from("./renders/alpha-lower-third")),
                preview: TapePreviewFile {
                    enabled: true,
                    frame: Some(48),
                    args: Vec::new(),
                },
                aesthetic: Some(TapeAesthetic {
                    label_style: Some(String::from("clean")),
                    shell_colorway: Some(String::from("black")),
                }),
                notes: Some(String::from("Broadcast-safe lower third with alpha.")),
            },
            TapeFile {
                id: String::from("neon-title"),
                name: Some(String::from("Neon Title Card")),
                manifest: String::from("./manifests/vcr_welcome_card.vcr"),
                mode: TapeMode::Video,
                primary_args: Vec::new(),
                output_dir: Some(String::from("./renders/neon-title")),
                preview: TapePreviewFile {
                    enabled: true,
                    frame: Some(60),
                    args: Vec::new(),
                },
                aesthetic: Some(TapeAesthetic {
                    label_style: Some(String::from("noisy")),
                    shell_colorway: Some(String::from("gray")),
                }),
                notes: Some(String::from("CRT glow title card.")),
            },
            TapeFile {
                id: String::from("frame-poster"),
                name: Some(String::from("Poster Frame")),
                manifest: String::from("./manifests/hello.vcr"),
                mode: TapeMode::Frame,
                primary_args: vec![String::from("--frame"), String::from("120")],
                output_dir: Some(String::from("./renders/frame-poster")),
                preview: TapePreviewFile {
                    enabled: true,
                    frame: Some(120),
                    args: Vec::new(),
                },
                aesthetic: Some(TapeAesthetic {
                    label_style: Some(String::from("handwritten")),
                    shell_colorway: Some(String::from("clear")),
                }),
                notes: Some(String::from("Still frame export profile.")),
            },
            TapeFile {
                id: String::from("pack-y2k"),
                name: Some(String::from("Y2K Showcase")),
                manifest: String::from("./manifests/examples/y2k_showcase.vcr"),
                mode: TapeMode::Video,
                primary_args: Vec::new(),
                output_dir: Some(String::from("./renders/pack-y2k")),
                preview: TapePreviewFile {
                    enabled: true,
                    frame: Some(30),
                    args: Vec::new(),
                },
                aesthetic: Some(TapeAesthetic {
                    label_style: Some(String::from("noisy")),
                    shell_colorway: Some(String::from("black")),
                }),
                notes: Some(String::from("Pack-driven Y2K validation run.")),
            },
            TapeFile {
                id: String::from("debug-safe-mode"),
                name: Some(String::from("Debug Safe Mode")),
                manifest: String::from("./manifests/hello.vcr"),
                mode: TapeMode::Frame,
                primary_args: vec![String::from("--frame"), String::from("0")],
                output_dir: Some(String::from("./renders/debug-safe-mode")),
                preview: TapePreviewFile {
                    enabled: true,
                    frame: Some(0),
                    args: Vec::new(),
                },
                aesthetic: Some(TapeAesthetic {
                    label_style: Some(String::from("clean")),
                    shell_colorway: Some(String::from("gray")),
                }),
                notes: Some(String::from("Deterministic smoke-check frame.")),
            },
        ],
    }
}

pub fn write_starter_config(
    config_path: &Path,
    launch_cwd: &Path,
    overwrite: bool,
) -> Result<TapeStore> {
    if config_path.exists() && !overwrite {
        bail!(
            "tape config already exists at {} (use --config to choose another path)",
            config_path.display()
        );
    }
    let file = starter_store_file(launch_cwd);
    save_store_file(config_path, &file)?;
    let store = resolve_store(file, config_path, launch_cwd)?;
    fs::create_dir_all(&store.runs_dir)
        .with_context(|| format!("failed to create runs_dir {}", store.runs_dir.display()))?;
    Ok(store)
}

pub fn append_stub_tape(file: &mut TapeStoreFile, tape_id: &str) -> Result<()> {
    let tape_id = tape_id.trim();
    if tape_id.is_empty() {
        bail!("tape id must not be empty");
    }
    if file.tapes.iter().any(|tape| tape.id == tape_id) {
        bail!("tape '{}' already exists", tape_id);
    }
    file.tapes.push(TapeFile {
        id: tape_id.to_owned(),
        name: Some(tape_id.to_owned()),
        manifest: format!("./manifests/{tape_id}.vcr"),
        mode: TapeMode::Video,
        primary_args: Vec::new(),
        output_dir: Some(format!("./renders/{tape_id}")),
        preview: TapePreviewFile::default(),
        aesthetic: None,
        notes: Some(String::from("Describe this tape.")),
    });
    Ok(())
}

pub fn build_command_plan(
    store: &TapeStore,
    tape: &Tape,
    action: TapeAction,
    dry_run: bool,
    timestamp: DateTime<Local>,
) -> Result<TapeCommandPlan> {
    if action == TapeAction::Preview && !tape.preview.enabled {
        bail!("tape '{}' does not have preview enabled", tape.id);
    }

    let run_id = next_run_id(&tape.id, &timestamp);
    let mut args = Vec::new();
    let source_args = if action == TapeAction::Primary {
        tape.primary_args.clone()
    } else {
        tape.preview.args.clone()
    };
    let explicit_subcommand = has_subcommand(&source_args);

    if explicit_subcommand {
        args.extend(source_args);
    } else {
        match action {
            TapeAction::Primary => {
                if tape.mode == TapeMode::Frame {
                    args.push(String::from("render-frame"));
                    args.push(tape.resolved_manifest_path.to_string_lossy().to_string());
                    args.extend(source_args);
                    if !has_frame_flag(&args) {
                        args.push(String::from("--frame"));
                        args.push(String::from("0"));
                    }
                } else {
                    args.push(String::from("render"));
                    args.push(tape.resolved_manifest_path.to_string_lossy().to_string());
                    args.extend(source_args);
                }
            }
            TapeAction::Preview => {
                args.push(String::from("render-frame"));
                args.push(tape.resolved_manifest_path.to_string_lossy().to_string());
                args.extend(source_args);
                if !has_frame_flag(&args) {
                    args.push(String::from("--frame"));
                    args.push(tape.preview.frame.to_string());
                }
            }
        }
    }

    let mut output_paths = extract_output_paths(&args, &store.output_flag, &store.project_root);
    if output_paths.is_empty() && !has_output_flag(&args, &store.output_flag) {
        let extension = if action == TapeAction::Preview || tape.mode == TapeMode::Frame {
            "png"
        } else {
            "mov"
        };
        let suffix = if action == TapeAction::Preview {
            "_preview"
        } else {
            ""
        };
        let output_path = tape
            .output_dir
            .join(format!("{run_id}{suffix}.{extension}"));
        args.push(store.output_flag.clone());
        args.push(output_path.to_string_lossy().to_string());
        output_paths.push(output_path);
    }

    let record_path = store
        .runs_dir
        .join("records")
        .join(format!("{run_id}.json"));

    Ok(TapeCommandPlan {
        timestamp,
        run_id,
        binary: store.vcr_binary.clone(),
        args,
        cwd: store.project_root.clone(),
        env_overrides: store.env.clone(),
        manifest_path: tape.resolved_manifest_path.clone(),
        output_paths,
        action,
        dry_run,
        record_path,
        explicit_subcommand,
    })
}

pub fn detect_features(binary: &str, cwd: &Path) -> FeatureProbe {
    let key = format!("{}|{}", binary, cwd.display());
    let cache = FEATURE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(&key) {
            return cached.clone();
        }
    }

    let mut command = Command::new(binary);
    command.arg("--help").current_dir(cwd);
    let probe = match command.output() {
        Ok(output) => {
            let mut merged = String::new();
            merged.push_str(&String::from_utf8_lossy(&output.stdout));
            merged.push_str(&String::from_utf8_lossy(&output.stderr));
            let help_snippet = merged.chars().take(220).collect::<String>();
            FeatureProbe {
                checked: true,
                has_render_frame: merged.contains("render-frame"),
                help_snippet,
                detection_failure: if output.status.success() {
                    None
                } else {
                    Some(format!(
                        "{} --help exited with status {:#?}",
                        binary,
                        output.status.code()
                    ))
                },
            }
        }
        Err(error) => FeatureProbe {
            checked: true,
            has_render_frame: false,
            help_snippet: String::new(),
            detection_failure: Some(format!("failed to run '{} --help': {error}", binary)),
        },
    };

    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, probe.clone());
    }

    probe
}

pub fn ensure_preview_supported(
    store: &TapeStore,
    plan: &TapeCommandPlan,
    action: TapeAction,
) -> Result<()> {
    if action != TapeAction::Preview || plan.explicit_subcommand {
        return Ok(());
    }

    let probe = detect_features(&store.vcr_binary, &store.project_root);
    if probe.has_render_frame {
        return Ok(());
    }

    let mut message = String::from(
        "Preview requires render-frame. Update VCR or define preview args as an explicit supported subcommand.",
    );
    if let Some(failure) = probe.detection_failure {
        message.push_str(" Feature probe failed: ");
        message.push_str(&failure);
    }
    bail!(message)
}

pub fn plan_to_run_record(
    tape: &Tape,
    plan: &TapeCommandPlan,
    exit_code: i32,
    record_path: &Path,
) -> RunRecord {
    RunRecord {
        timestamp: plan.timestamp.to_rfc3339(),
        run_id: plan.run_id.clone(),
        tape_id: tape.id.clone(),
        tape_name: tape.name.clone(),
        resolved_manifest_path: plan.manifest_path.to_string_lossy().to_string(),
        resolved_command: {
            let mut command = vec![plan.binary.clone()];
            command.extend(plan.args.clone());
            command
        },
        cwd: plan.cwd.to_string_lossy().to_string(),
        env_overrides: plan.env_overrides.clone(),
        exit_code,
        output_paths: plan
            .output_paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
        action: plan.action.as_str().to_owned(),
        dry_run: plan.dry_run,
        record_path_written: record_path.to_string_lossy().to_string(),
    }
}

pub fn write_run_record(record_path: &Path, record: &RunRecord) -> Result<()> {
    if let Some(parent) = record_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create run record directory {}", parent.display())
        })?;
    }
    let serialized =
        serde_json::to_string_pretty(record).context("failed to serialize run record")?;
    fs::write(record_path, format!("{serialized}\n"))
        .with_context(|| format!("failed to write run record {}", record_path.display()))?;
    Ok(())
}

pub fn run_tape_action(
    store: &TapeStore,
    tape_id: &str,
    action: TapeAction,
    dry_run: bool,
) -> Result<RunOutcome> {
    let tape = store
        .tape_by_id(tape_id)
        .ok_or_else(|| anyhow!("unknown tape '{}'", tape_id))?;
    let timestamp = Local::now();
    let plan = build_command_plan(store, tape, action, dry_run, timestamp)?;
    ensure_preview_supported(store, &plan, action)?;

    fs::create_dir_all(&store.runs_dir)
        .with_context(|| format!("failed to create runs_dir {}", store.runs_dir.display()))?;
    fs::create_dir_all(&tape.output_dir).with_context(|| {
        format!(
            "failed to create tape output_dir {}",
            tape.output_dir.display()
        )
    })?;
    for output_path in &plan.output_paths {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create output directory {}", parent.display())
            })?;
        }
    }

    let exit_code = if dry_run {
        println!("[dry-run] {} {}", plan.binary, plan.args.join(" "));
        0
    } else {
        let mut command = Command::new(&plan.binary);
        command
            .args(&plan.args)
            .current_dir(&plan.cwd)
            .envs(plan.env_overrides.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to spawn tape command: {} {}",
                plan.binary,
                plan.args.join(" ")
            )
        })?;

        let stdout_handle = child.stdout.take().map(|stdout| {
            std::thread::spawn(move || -> Result<()> {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    let read = reader.read_line(&mut line)?;
                    if read == 0 {
                        break;
                    }
                    print!("{line}");
                    std::io::stdout().flush()?;
                }
                Ok(())
            })
        });

        let stderr_handle = child.stderr.take().map(|stderr| {
            std::thread::spawn(move || -> Result<()> {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    let read = reader.read_line(&mut line)?;
                    if read == 0 {
                        break;
                    }
                    eprint!("{line}");
                    std::io::stderr().flush()?;
                }
                Ok(())
            })
        });

        let status = child
            .wait()
            .context("failed while waiting for tape command")?;
        let exit_code = status.code().unwrap_or(1);

        if let Some(handle) = stdout_handle {
            let _ = handle.join();
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.join();
        }
        exit_code
    };

    let record = plan_to_run_record(tape, &plan, exit_code, &plan.record_path);
    write_run_record(&plan.record_path, &record)?;

    if exit_code != 0 {
        bail!(
            "tape '{}' {} failed with exit code {} (record: {})",
            tape.id,
            action.as_str(),
            exit_code,
            plan.record_path.display()
        );
    }

    Ok(RunOutcome {
        first_output_path: plan.output_paths.first().cloned(),
        record_path: plan.record_path,
        record,
    })
}

pub fn run_doctor(store: &TapeStore) -> DoctorReport {
    let mut diagnostics = Vec::new();
    let mut ok = true;

    diagnostics.push(format!(
        "[ok] Loaded tape config: {}",
        store.config_path.display()
    ));
    diagnostics.push(String::from(
        "[ok] Path rules: project_root resolves relative to config dir; manifests resolve relative to project_root; runs_dir resolves relative to config dir.",
    ));

    let binary_probe = Command::new(&store.vcr_binary)
        .arg("--help")
        .current_dir(&store.project_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match binary_probe {
        Ok(status) if status.success() => diagnostics.push(format!(
            "[ok] VCR binary is executable: {}",
            store.vcr_binary
        )),
        Ok(status) => {
            ok = false;
            diagnostics.push(format!(
                "[error] '{}' is reachable but '--help' exited with status {:?}.",
                store.vcr_binary,
                status.code()
            ));
        }
        Err(error) => {
            ok = false;
            diagnostics.push(format!(
                "[error] Failed to execute '{} --help': {}",
                store.vcr_binary, error
            ));
        }
    }

    if store.tapes.is_empty() {
        ok = false;
        diagnostics.push(String::from(
            "[error] Config has no tapes. Add one with: vcr tape new <id>",
        ));
    }

    for tape in &store.tapes {
        if tape.resolved_manifest_path.exists() {
            diagnostics.push(format!(
                "[ok] tape '{}' manifest exists: {}",
                tape.id,
                tape.resolved_manifest_path.display()
            ));
        } else {
            ok = false;
            diagnostics.push(format!(
                "[error] tape '{}' manifest missing: {}",
                tape.id,
                tape.resolved_manifest_path.display()
            ));
        }
    }

    if store
        .tapes
        .iter()
        .any(|tape| tape.preview.enabled && !has_subcommand(&tape.preview.args))
    {
        let probe = detect_features(&store.vcr_binary, &store.project_root);
        if probe.has_render_frame {
            diagnostics.push(String::from(
                "[ok] render-frame detected for preview-enabled tapes.",
            ));
        } else {
            ok = false;
            diagnostics.push(String::from(
                "[error] Preview requires render-frame. Update VCR or define preview args as an explicit supported subcommand.",
            ));
        }
    }

    DoctorReport { ok, diagnostics }
}

pub fn render_tape_table(store: &TapeStore) -> String {
    if store.tapes.is_empty() {
        return String::from("No tapes configured.");
    }

    let mut id_width = "id".len();
    let mut name_width = "name".len();
    let mut mode_width = "mode".len();
    let mut preview_width = "preview".len();

    for tape in &store.tapes {
        id_width = id_width.max(tape.id.len());
        name_width = name_width.max(tape.name.len());
        mode_width = mode_width.max(match tape.mode {
            TapeMode::Video => "video".len(),
            TapeMode::Frame => "frame".len(),
        });
        preview_width = preview_width.max(if tape.preview.enabled { 3 } else { 2 });
    }

    let mut output = String::new();
    output.push_str(&format!(
        "{:<id_width$}  {:<name_width$}  {:<mode_width$}  {:<preview_width$}  {}\n",
        "id", "name", "mode", "preview", "manifest"
    ));
    output.push_str(&format!(
        "{}  {}  {}  {}  {}\n",
        "-".repeat(id_width),
        "-".repeat(name_width),
        "-".repeat(mode_width),
        "-".repeat(preview_width),
        "-".repeat(8)
    ));

    for tape in &store.tapes {
        let mode = match tape.mode {
            TapeMode::Video => "video",
            TapeMode::Frame => "frame",
        };
        let preview = if tape.preview.enabled { "yes" } else { "no" };
        output.push_str(&format!(
            "{:<id_width$}  {:<name_width$}  {:<mode_width$}  {:<preview_width$}  {}\n",
            tape.id, tape.name, mode, preview, tape.manifest
        ));
    }

    output.trim_end().to_owned()
}

pub fn find_tape_line_number(config_path: &Path, tape_id: &str) -> Result<Option<usize>> {
    let content = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read tape config {}", config_path.display()))?;
    let pattern = Regex::new(&format!(
        r#"^\s*id:\s*["']?{}["']?\s*$"#,
        regex::escape(tape_id)
    ))
    .context("failed to build tape id regex")?;

    for (index, line) in content.lines().enumerate() {
        if pattern.is_match(line) {
            return Ok(Some(index + 1));
        }
    }

    Ok(None)
}

pub fn open_editor_at_tape(config_path: &Path, tape_id: &str) -> Result<()> {
    let line = find_tape_line_number(config_path, tape_id)?;

    let editor = env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| String::from("vi"));

    let mut parts = split_command_words(&editor);
    if parts.is_empty() {
        parts.push(String::from("vi"));
    }

    let program = parts.remove(0);
    let mut args = parts;
    if is_vi_family(&program) {
        if let Some(line_number) = line {
            args.push(format!("+{line_number}"));
        }
    }
    args.push(config_path.to_string_lossy().to_string());

    let status = Command::new(&program)
        .args(&args)
        .status()
        .with_context(|| format!("failed to launch editor '{}': {}", program, editor))?;

    if !status.success() {
        bail!(
            "editor '{}' exited with status {:?}",
            program,
            status.code()
        );
    }

    Ok(())
}

pub fn deck_command(binary: &str, config_path: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("run")
        .arg("--config")
        .arg(config_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

pub fn run_deck(config_path: &Path) -> Result<()> {
    let candidates = ["tape-deck", "vhs"];

    let mut first_non_not_found: Option<anyhow::Error> = None;
    for candidate in candidates {
        let status_result = deck_command(candidate, config_path).status();
        match status_result {
            Ok(status) => {
                if status.success() {
                    return Ok(());
                }
                return Err(anyhow!(
                    "deck command '{}' exited with status {:?}",
                    candidate,
                    status.code()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                continue;
            }
            Err(error) => {
                first_non_not_found = Some(anyhow!(
                    "failed to launch deck command '{}': {}",
                    candidate,
                    error
                ));
                break;
            }
        }
    }

    if let Some(error) = first_non_not_found {
        return Err(error);
    }

    bail!(
        "No deck binary found on PATH. Install one and retry:\n\
         1) cd {}\n\
         2) go build -o ~/.local/bin/tape-deck ./cmd/tape-deck\n\
         3) tape-deck --help\n\
         Then run: vcr deck --config {}",
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("vhs-tape-deck")
            .display(),
        config_path.display()
    )
}

fn has_subcommand(args: &[String]) -> bool {
    args.iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| !value.starts_with('-'))
        .unwrap_or(false)
}

fn has_output_flag(args: &[String], output_flag: &str) -> bool {
    args.iter().enumerate().any(|(index, arg)| {
        if arg == output_flag {
            return index + 1 < args.len();
        }
        arg.starts_with(&format!("{output_flag}="))
    })
}

fn has_frame_flag(args: &[String]) -> bool {
    args.iter().enumerate().any(|(index, arg)| {
        if arg == "--frame" {
            return index + 1 < args.len();
        }
        arg.starts_with("--frame=")
    })
}

fn extract_output_paths(args: &[String], output_flag: &str, cwd: &Path) -> Vec<PathBuf> {
    let mut outputs = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if arg == output_flag {
            if let Some(value) = args.get(index + 1) {
                outputs.push(resolve_output_path(value, cwd));
            }
            index += 2;
            continue;
        }
        let prefix = format!("{output_flag}=");
        if let Some(value) = arg.strip_prefix(&prefix) {
            outputs.push(resolve_output_path(value, cwd));
        }
        index += 1;
    }
    outputs
}

fn resolve_output_path(path: &str, cwd: &Path) -> PathBuf {
    if let Ok(resolved) = resolve_string_path(path, cwd) {
        resolved
    } else {
        cwd.join(path)
    }
}

fn next_run_id(tape_id: &str, timestamp: &DateTime<Local>) -> String {
    let formatted = timestamp.format("%Y%m%d_%H%M%S").to_string();
    let counters = RUN_ID_COUNTERS.get_or_init(|| Mutex::new(HashMap::new()));
    let count = if let Ok(mut guard) = counters.lock() {
        let entry = guard.entry(tape_id.to_owned()).or_insert(0);
        *entry += 1;
        *entry
    } else {
        1
    };
    format!("{formatted}_{tape_id}_{count:03}")
}

fn split_command_words(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>()
}

fn is_vi_family(program: &str) -> bool {
    let file_stem = Path::new(program)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(file_stem.as_str(), "vi" | "vim" | "nvim")
}

fn resolve_string_path(value: &str, base_dir: &Path) -> Result<PathBuf> {
    let expanded = expand_home(value)?;
    resolve_against_base_path(&expanded, base_dir)
}

fn resolve_against_base_path(path: &Path, base_dir: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    let absolute = if joined.is_absolute() {
        joined
    } else {
        env::current_dir()
            .context("failed to resolve current directory")?
            .join(joined)
    };
    Ok(normalize_path(absolute))
}

fn expand_home(value: &str) -> Result<PathBuf> {
    if !value.starts_with('~') {
        return Ok(PathBuf::from(value));
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
        .ok_or_else(|| anyhow!("cannot resolve home directory for path '{value}'"))?;

    if value == "~" {
        return Ok(home);
    }

    if value.starts_with("~/") || value.starts_with("~\\") {
        return Ok(home.join(&value[2..]));
    }

    bail!("unsupported home path syntax: {value}")
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn user_config_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(path) = env::var_os("APPDATA") {
            return Ok(PathBuf::from(path));
        }
        if let Some(home) = env::var_os("USERPROFILE") {
            return Ok(PathBuf::from(home).join("AppData").join("Roaming"));
        }
        bail!("unable to resolve user config directory on Windows");
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join("Library")
                .join("Application Support"));
        }
        bail!("unable to resolve HOME for macOS config directory");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(path));
        }
        if let Some(home) = env::var_os("HOME") {
            return Ok(PathBuf::from(home).join(".config"));
        }
        bail!("unable to resolve user config directory on unix");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file() -> TapeStoreFile {
        TapeStoreFile {
            vcr_binary: None,
            output_flag: None,
            project_root: Some(String::from("./project")),
            runs_dir: None,
            env: BTreeMap::from([(String::from("VCR_SEED"), String::from("0"))]),
            tapes: vec![TapeFile {
                id: String::from("alpha"),
                name: None,
                manifest: String::from("./manifests/alpha.vcr"),
                mode: TapeMode::Frame,
                primary_args: vec![String::from("--frame"), String::from("6")],
                output_dir: None,
                preview: TapePreviewFile {
                    enabled: true,
                    frame: Some(9),
                    args: Vec::new(),
                },
                aesthetic: None,
                notes: None,
            }],
        }
    }

    #[test]
    fn resolve_store_applies_defaults_and_resolves_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("cfg");
        fs::create_dir_all(&config_dir).expect("create cfg dir");
        let config_path = config_dir.join("tapes.yaml");

        let launch_cwd = temp.path().join("workspace");
        fs::create_dir_all(&launch_cwd).expect("create launch cwd");

        let store = resolve_store(sample_file(), &config_path, &launch_cwd).expect("resolve store");
        assert_eq!(store.vcr_binary, "vcr");
        assert_eq!(store.output_flag, "--output");
        assert_eq!(store.project_root, temp.path().join("cfg/project"));
        assert_eq!(store.runs_dir, temp.path().join("cfg/runs"));
        assert_eq!(
            store.tapes[0].output_dir,
            temp.path().join("cfg/runs/alpha")
        );
        assert_eq!(
            store.tapes[0].resolved_manifest_path,
            temp.path().join("cfg/project/manifests/alpha.vcr")
        );
        assert_eq!(store.tapes[0].name, "alpha");
    }

    #[test]
    fn append_stub_tape_rejects_duplicates() {
        let mut file = TapeStoreFile::default();
        append_stub_tape(&mut file, "alpha").expect("append alpha");
        let duplicate_error =
            append_stub_tape(&mut file, "alpha").expect_err("duplicate id should fail");
        assert!(duplicate_error.to_string().contains("already exists"));
    }

    #[test]
    fn build_command_plan_does_not_double_inject_frame_for_primary_frame_tape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("tapes.yaml");
        let launch_cwd = temp.path().join("workspace");
        fs::create_dir_all(&launch_cwd).expect("create cwd");

        let store = resolve_store(sample_file(), &config_path, &launch_cwd).expect("resolve store");
        let tape = store.tape_by_id("alpha").expect("alpha tape");

        let plan = build_command_plan(&store, tape, TapeAction::Primary, false, Local::now())
            .expect("build plan");

        let frame_flags = plan
            .args
            .iter()
            .filter(|value| *value == "--frame" || value.starts_with("--frame="))
            .count();
        assert_eq!(frame_flags, 1);
        assert!(!plan
            .args
            .windows(2)
            .any(|window| window == ["--frame", "0"]));
    }

    #[test]
    fn build_command_plan_injects_configured_output_flag() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("tapes.yaml");
        let launch_cwd = temp.path().join("workspace");
        fs::create_dir_all(&launch_cwd).expect("create cwd");

        let mut file = sample_file();
        file.output_flag = Some(String::from("--out"));
        let store = resolve_store(file, &config_path, &launch_cwd).expect("resolve store");
        let tape = store.tape_by_id("alpha").expect("alpha tape");

        let plan = build_command_plan(&store, tape, TapeAction::Primary, false, Local::now())
            .expect("build plan");

        assert!(plan.args.contains(&String::from("--out")));
        assert!(!plan.args.contains(&String::from("--output")));
    }

    #[test]
    fn preview_gate_fails_without_render_frame_when_preview_uses_default_render_frame() {
        let temp = tempfile::tempdir().expect("tempdir");
        let launch_cwd = temp.path().join("workspace");
        fs::create_dir_all(&launch_cwd).expect("create cwd");

        let store = TapeStore {
            config_path: temp.path().join("tapes.yaml"),
            config_dir: temp.path().to_path_buf(),
            vcr_binary: String::from("definitely-not-a-binary"),
            output_flag: String::from("--output"),
            project_root: launch_cwd.clone(),
            runs_dir: temp.path().join("runs"),
            env: BTreeMap::new(),
            tapes: vec![Tape {
                id: String::from("alpha"),
                name: String::from("alpha"),
                manifest: String::from("./manifests/alpha.vcr"),
                resolved_manifest_path: launch_cwd.join("manifests/alpha.vcr"),
                mode: TapeMode::Video,
                primary_args: Vec::new(),
                output_dir: launch_cwd.join("renders/alpha"),
                preview: TapePreview {
                    enabled: true,
                    frame: 0,
                    args: Vec::new(),
                },
                aesthetic: None,
                notes: None,
            }],
        };

        let tape = store.tape_by_id("alpha").expect("alpha tape");
        let plan = build_command_plan(&store, tape, TapeAction::Preview, false, Local::now())
            .expect("build preview plan");

        let error = ensure_preview_supported(&store, &plan, TapeAction::Preview)
            .expect_err("preview should fail without render-frame support");
        assert!(error.to_string().contains("Preview requires render-frame"));
    }

    #[test]
    fn run_record_serialization_contains_required_fields() {
        let record = RunRecord {
            timestamp: String::from("2026-02-20T12:30:01Z"),
            run_id: String::from("20260220_123001_alpha_001"),
            tape_id: String::from("alpha"),
            tape_name: String::from("Alpha"),
            resolved_manifest_path: String::from("/tmp/project/manifests/alpha.vcr"),
            resolved_command: vec![
                String::from("vcr"),
                String::from("render"),
                String::from("/tmp/project/manifests/alpha.vcr"),
            ],
            cwd: String::from("/tmp/project"),
            env_overrides: BTreeMap::from([(String::from("VCR_SEED"), String::from("0"))]),
            exit_code: 0,
            output_paths: vec![String::from("/tmp/project/renders/alpha.mov")],
            action: String::from("primary"),
            dry_run: false,
            record_path_written: String::from("/tmp/runs/records/20260220_123001_alpha_001.json"),
        };

        let json = serde_json::to_string_pretty(&record).expect("serialize");
        assert!(json.contains("\"run_id\""));
        assert!(json.contains("\"resolved_manifest_path\""));
        assert!(json.contains("\"resolved_command\""));
        assert!(json.contains("\"record_path_written\""));
    }
}
