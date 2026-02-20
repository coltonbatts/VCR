package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestApplyDefaults(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	cfgPath := filepath.Join(tmp, "config.yaml")

	cfg := &Config{
		Tapes: []Tape{{
			ID:       "alpha",
			Manifest: "./manifests/alpha.yaml",
			Mode:     ModeVideo,
			Preview:  Preview{Enabled: true, Frame: 12},
		}},
	}

	if err := ApplyDefaults(cfg, cfgPath, "/workspace/project"); err != nil {
		t.Fatalf("ApplyDefaults: %v", err)
	}

	if cfg.VCRBinary != "vcr" {
		t.Fatalf("unexpected vcr binary: %s", cfg.VCRBinary)
	}
	if cfg.OutputFlag != "--output" {
		t.Fatalf("unexpected output flag default: %s", cfg.OutputFlag)
	}
	if cfg.ProjectRoot != "/workspace/project" {
		t.Fatalf("unexpected project root: %s", cfg.ProjectRoot)
	}
	if cfg.RunsDir != filepath.Join(tmp, "runs") {
		t.Fatalf("unexpected runs dir: %s", cfg.RunsDir)
	}
	if cfg.Tapes[0].OutputDir != filepath.Join(tmp, "runs", "alpha") {
		t.Fatalf("unexpected output dir: %s", cfg.Tapes[0].OutputDir)
	}
}

func TestLoadAndValidateDuplicateTapeID(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	cfgPath := filepath.Join(tmp, "config.yaml")

	data := `tapes:
  - id: dup
    name: First
    manifest: ./manifests/a.yaml
    mode: video
    preview: {enabled: false}
  - id: dup
    name: Second
    manifest: ./manifests/b.yaml
    mode: frame
    preview: {enabled: true, frame: 0}
`
	if err := os.WriteFile(cfgPath, []byte(data), 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}

	_, err := Load(cfgPath, tmp)
	if err == nil {
		t.Fatal("expected duplicate tape id error")
	}
	if !strings.Contains(err.Error(), "duplicate tape id") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestResolvePath(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	resolved, err := ResolvePath("./foo/../bar", tmp)
	if err != nil {
		t.Fatalf("ResolvePath: %v", err)
	}
	expected := filepath.Join(tmp, "bar")
	if resolved != expected {
		t.Fatalf("expected %s got %s", expected, resolved)
	}
}

func TestWriteStarterConfig(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	cfgPath := filepath.Join(tmp, "conf", "config.yaml")

	if err := WriteStarterConfig(cfgPath, tmp, false); err != nil {
		t.Fatalf("WriteStarterConfig: %v", err)
	}

	cfg, err := Load(cfgPath, tmp)
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if len(cfg.Tapes) != 5 {
		t.Fatalf("expected 5 tapes, got %d", len(cfg.Tapes))
	}
	if cfg.OutputFlag != "--output" {
		t.Fatalf("expected starter output_flag --output, got %s", cfg.OutputFlag)
	}
}

func TestValidateOutputFlag(t *testing.T) {
	t.Parallel()

	cfg := &Config{
		OutputFlag:  "output",
		VCRBinary:   "vcr",
		ProjectRoot: "/tmp/project",
		RunsDir:     "/tmp/runs",
		Tapes: []Tape{{
			ID:       "alpha",
			Name:     "Alpha",
			Manifest: "./manifests/alpha.yaml",
			Mode:     ModeVideo,
			Preview:  Preview{Enabled: false},
			Aesthetic: Aesthetic{
				LabelStyle:    LabelStyleClean,
				ShellColorway: ShellColorwayBlack,
			},
		}},
	}

	if err := Validate(cfg); err == nil {
		t.Fatalf("expected output_flag validation error")
	}
}
