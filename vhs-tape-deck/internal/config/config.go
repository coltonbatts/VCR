package config

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"gopkg.in/yaml.v3"
)

const (
	DefaultAppDirName = "vhs-tape-deck"
	DefaultConfigName = "config.yaml"
)

type Mode string

const (
	ModeVideo Mode = "video"
	ModeFrame Mode = "frame"
)

type LabelStyle string

const (
	LabelStyleClean       LabelStyle = "clean"
	LabelStyleNoisy       LabelStyle = "noisy"
	LabelStyleHandwritten LabelStyle = "handwritten"
)

type ShellColorway string

const (
	ShellColorwayBlack ShellColorway = "black"
	ShellColorwayGray  ShellColorway = "gray"
	ShellColorwayClear ShellColorway = "clear"
)

type Config struct {
	VCRBinary   string            `yaml:"vcr_binary"`
	OutputFlag  string            `yaml:"output_flag"`
	ProjectRoot string            `yaml:"project_root"`
	RunsDir     string            `yaml:"runs_dir"`
	Env         map[string]string `yaml:"env"`
	Tapes       []Tape            `yaml:"tapes"`
}

type Tape struct {
	ID          string    `yaml:"id"`
	Name        string    `yaml:"name"`
	Manifest    string    `yaml:"manifest"`
	Mode        Mode      `yaml:"mode"`
	PrimaryArgs []string  `yaml:"primary_args"`
	OutputDir   string    `yaml:"output_dir,omitempty"`
	Preview     Preview   `yaml:"preview"`
	Aesthetic   Aesthetic `yaml:"aesthetic,omitempty"`
	Notes       string    `yaml:"notes,omitempty"`
}

type Preview struct {
	Enabled bool     `yaml:"enabled"`
	Frame   int      `yaml:"frame,omitempty"`
	Args    []string `yaml:"args,omitempty"`
}

type Aesthetic struct {
	LabelStyle    LabelStyle    `yaml:"label_style,omitempty"`
	ShellColorway ShellColorway `yaml:"shell_colorway,omitempty"`
}

func DefaultConfigPath() (string, error) {
	base, err := os.UserConfigDir()
	if err != nil {
		return "", fmt.Errorf("resolve user config dir: %w", err)
	}
	return filepath.Join(base, DefaultAppDirName, DefaultConfigName), nil
}

func ConfigDir(configPath string) string {
	return filepath.Dir(configPath)
}

func Load(configPath, launchCWD string) (*Config, error) {
	buf, err := os.ReadFile(configPath)
	if err != nil {
		return nil, fmt.Errorf("read config: %w", err)
	}
	var cfg Config
	if err := yaml.Unmarshal(buf, &cfg); err != nil {
		return nil, fmt.Errorf("parse yaml: %w", err)
	}
	if err := ApplyDefaults(&cfg, configPath, launchCWD); err != nil {
		return nil, err
	}
	return &cfg, nil
}

func ApplyDefaults(cfg *Config, configPath, launchCWD string) error {
	if cfg == nil {
		return errors.New("config is nil")
	}
	if strings.TrimSpace(launchCWD) == "" {
		cwd, err := os.Getwd()
		if err != nil {
			return fmt.Errorf("resolve cwd: %w", err)
		}
		launchCWD = cwd
	}
	configDir := ConfigDir(configPath)

	if strings.TrimSpace(cfg.VCRBinary) == "" {
		cfg.VCRBinary = "vcr"
	}
	cfg.OutputFlag = strings.TrimSpace(cfg.OutputFlag)
	if cfg.OutputFlag == "" {
		cfg.OutputFlag = "--output"
	}

	if strings.TrimSpace(cfg.ProjectRoot) == "" {
		cfg.ProjectRoot = launchCWD
	}
	projectRoot, err := ResolvePath(cfg.ProjectRoot, configDir)
	if err != nil {
		return fmt.Errorf("resolve project_root: %w", err)
	}
	cfg.ProjectRoot = projectRoot

	if strings.TrimSpace(cfg.RunsDir) == "" {
		cfg.RunsDir = filepath.Join(configDir, "runs")
	}
	runsDir, err := ResolvePath(cfg.RunsDir, configDir)
	if err != nil {
		return fmt.Errorf("resolve runs_dir: %w", err)
	}
	cfg.RunsDir = runsDir

	if cfg.Env == nil {
		cfg.Env = map[string]string{}
	}

	for i := range cfg.Tapes {
		t := &cfg.Tapes[i]
		if strings.TrimSpace(t.Name) == "" {
			t.Name = t.ID
		}
		if strings.TrimSpace(t.OutputDir) == "" {
			t.OutputDir = filepath.Join(cfg.RunsDir, t.ID)
		}
		resolvedOutputDir, err := ResolvePath(t.OutputDir, cfg.ProjectRoot)
		if err != nil {
			return fmt.Errorf("resolve output_dir for %q: %w", t.ID, err)
		}
		t.OutputDir = resolvedOutputDir

		if t.Preview.Frame < 0 {
			t.Preview.Frame = 0
		}

		if t.Aesthetic.LabelStyle == "" {
			t.Aesthetic.LabelStyle = LabelStyleClean
		}
		if t.Aesthetic.ShellColorway == "" {
			t.Aesthetic.ShellColorway = ShellColorwayBlack
		}
	}

	if err := Validate(cfg); err != nil {
		return err
	}
	return nil
}

func ResolveManifestPath(projectRoot, manifestPath string) (string, error) {
	return ResolvePath(manifestPath, projectRoot)
}

func ResolvePath(value, baseDir string) (string, error) {
	if strings.TrimSpace(value) == "" {
		return "", errors.New("empty path")
	}
	expanded, err := expandHome(value)
	if err != nil {
		return "", err
	}
	if !filepath.IsAbs(expanded) {
		expanded = filepath.Join(baseDir, expanded)
	}
	abs, err := filepath.Abs(expanded)
	if err != nil {
		return "", err
	}
	return filepath.Clean(abs), nil
}

func expandHome(pathValue string) (string, error) {
	if !strings.HasPrefix(pathValue, "~") {
		return pathValue, nil
	}

	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home dir: %w", err)
	}

	if pathValue == "~" {
		return home, nil
	}

	if strings.HasPrefix(pathValue, "~/") || strings.HasPrefix(pathValue, "~\\") {
		return filepath.Join(home, pathValue[2:]), nil
	}

	return "", fmt.Errorf("unsupported home path syntax: %q", pathValue)
}

func Validate(cfg *Config) error {
	if cfg == nil {
		return errors.New("config is nil")
	}
	outputFlag := strings.TrimSpace(cfg.OutputFlag)
	if outputFlag == "" {
		return errors.New("output_flag is required")
	}
	if !strings.HasPrefix(outputFlag, "-") {
		return fmt.Errorf("output_flag must start with '-': %q", cfg.OutputFlag)
	}
	if len(cfg.Tapes) == 0 {
		return errors.New("config requires at least one tape")
	}

	seen := map[string]struct{}{}
	validLabelStyles := map[LabelStyle]struct{}{
		LabelStyleClean:       {},
		LabelStyleNoisy:       {},
		LabelStyleHandwritten: {},
	}
	validShells := map[ShellColorway]struct{}{
		ShellColorwayBlack: {},
		ShellColorwayGray:  {},
		ShellColorwayClear: {},
	}

	for i, t := range cfg.Tapes {
		if strings.TrimSpace(t.ID) == "" {
			return fmt.Errorf("tapes[%d]: id is required", i)
		}
		if _, ok := seen[t.ID]; ok {
			return fmt.Errorf("duplicate tape id: %s", t.ID)
		}
		seen[t.ID] = struct{}{}

		if strings.TrimSpace(t.Manifest) == "" {
			return fmt.Errorf("tape %q: manifest is required", t.ID)
		}

		if t.Mode != ModeVideo && t.Mode != ModeFrame {
			return fmt.Errorf("tape %q: mode must be %q or %q", t.ID, ModeVideo, ModeFrame)
		}

		if t.Preview.Enabled && t.Preview.Frame < 0 {
			return fmt.Errorf("tape %q: preview frame must be >= 0", t.ID)
		}

		if _, ok := validLabelStyles[t.Aesthetic.LabelStyle]; !ok {
			values := make([]string, 0, len(validLabelStyles))
			for k := range validLabelStyles {
				values = append(values, string(k))
			}
			sort.Strings(values)
			return fmt.Errorf("tape %q: invalid label_style %q (valid: %s)", t.ID, t.Aesthetic.LabelStyle, strings.Join(values, ", "))
		}

		if _, ok := validShells[t.Aesthetic.ShellColorway]; !ok {
			values := make([]string, 0, len(validShells))
			for k := range validShells {
				values = append(values, string(k))
			}
			sort.Strings(values)
			return fmt.Errorf("tape %q: invalid shell_colorway %q (valid: %s)", t.ID, t.Aesthetic.ShellColorway, strings.Join(values, ", "))
		}
	}

	return nil
}

func WriteStarterConfig(configPath, launchCWD string, overwrite bool) error {
	if strings.TrimSpace(configPath) == "" {
		var err error
		configPath, err = DefaultConfigPath()
		if err != nil {
			return err
		}
	}

	if _, err := os.Stat(configPath); err == nil && !overwrite {
		return fmt.Errorf("config already exists at %s", configPath)
	} else if err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("check config path: %w", err)
	}

	cfg := StarterConfig(launchCWD)
	if err := ApplyDefaults(&cfg, configPath, launchCWD); err != nil {
		return err
	}

	if err := os.MkdirAll(filepath.Dir(configPath), 0o755); err != nil {
		return fmt.Errorf("create config dir: %w", err)
	}
	if err := os.MkdirAll(cfg.RunsDir, 0o755); err != nil {
		return fmt.Errorf("create runs dir: %w", err)
	}

	buf, err := yaml.Marshal(cfg)
	if err != nil {
		return fmt.Errorf("marshal config: %w", err)
	}
	if err := os.WriteFile(configPath, buf, 0o644); err != nil {
		return fmt.Errorf("write config: %w", err)
	}
	return nil
}

func StarterConfig(launchCWD string) Config {
	if strings.TrimSpace(launchCWD) == "" {
		launchCWD, _ = os.Getwd()
	}

	projectRoot := launchCWD
	if runtime.GOOS == "windows" {
		projectRoot = filepath.Clean(projectRoot)
	}

	return Config{
		VCRBinary:   "vcr",
		OutputFlag:  "--output",
		ProjectRoot: projectRoot,
		Env: map[string]string{
			"VCR_SEED": "0",
		},
		Tapes: []Tape{
			{
				ID:       "alpha-lower-third",
				Name:     "Alpha Lower Third",
				Manifest: "./manifests/alpha_lower_third.yaml",
				Mode:     ModeVideo,
				PrimaryArgs: []string{
					"--duration", "5",
					"--fps", "60",
				},
				Preview:   Preview{Enabled: true, Frame: 48, Args: []string{"--fps", "60"}},
				Aesthetic: Aesthetic{LabelStyle: LabelStyleClean, ShellColorway: ShellColorwayBlack},
				Notes:     "Broadcast-safe lower third with alpha",
			},
			{
				ID:       "neon-title",
				Name:     "Neon Title Card",
				Manifest: "./manifests/neon_title.yaml",
				Mode:     ModeVideo,
				PrimaryArgs: []string{
					"--duration", "6",
					"--fps", "60",
				},
				Preview:   Preview{Enabled: true, Frame: 90, Args: []string{"--quality", "draft"}},
				Aesthetic: Aesthetic{LabelStyle: LabelStyleNoisy, ShellColorway: ShellColorwayGray},
				Notes:     "CRT glow and scanline feel",
			},
			{
				ID:       "frame-poster",
				Name:     "Poster Frame",
				Manifest: "./manifests/poster_frame.yaml",
				Mode:     ModeFrame,
				PrimaryArgs: []string{
					"--frame", "160",
				},
				Preview:   Preview{Enabled: true, Frame: 120, Args: []string{"--quality", "draft"}},
				Aesthetic: Aesthetic{LabelStyle: LabelStyleHandwritten, ShellColorway: ShellColorwayClear},
				Notes:     "High detail still export",
			},
			{
				ID:       "pack-y2k",
				Name:     "Y2K Pack Probe",
				Manifest: "./manifests/pack_y2k.yaml",
				Mode:     ModeVideo,
				PrimaryArgs: []string{
					"--duration", "4",
					"--fps", "60",
					"--seed", "0",
				},
				Preview:   Preview{Enabled: true, Frame: 36, Args: []string{"--seed", "0"}},
				Aesthetic: Aesthetic{LabelStyle: LabelStyleNoisy, ShellColorway: ShellColorwayBlack},
				Notes:     "Pack-driven scene validation",
			},
			{
				ID:       "debug-safe-mode",
				Name:     "Debug Safe Mode",
				Manifest: "./manifests/debug_safe.yaml",
				Mode:     ModeFrame,
				PrimaryArgs: []string{
					"--frame", "0",
					"--seed", "0",
				},
				Preview:   Preview{Enabled: true, Frame: 0, Args: []string{"--seed", "0"}},
				Aesthetic: Aesthetic{LabelStyle: LabelStyleClean, ShellColorway: ShellColorwayGray},
				Notes:     "Deterministic sanity checks",
			},
		},
	}
}
