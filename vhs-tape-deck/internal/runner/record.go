package runner

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

type RunRecord struct {
	Timestamp    time.Time         `json:"timestamp"`
	RunID        string            `json:"run_id"`
	TapeID       string            `json:"tape_id"`
	TapeName     string            `json:"tape_name"`
	ManifestPath string            `json:"resolved_manifest_path"`
	Command      []string          `json:"command"`
	CWD          string            `json:"cwd"`
	EnvOverrides map[string]string `json:"env_overrides"`
	ExitCode     int               `json:"exit_code"`
	OutputPaths  []string          `json:"output_paths"`
	Action       Action            `json:"action"`
	DryRun       bool              `json:"dry_run"`
}

func WriteRunRecord(path string, record *RunRecord) error {
	if record == nil {
		return fmt.Errorf("nil run record")
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("mkdir record dir: %w", err)
	}
	buf, err := json.MarshalIndent(record, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal record: %w", err)
	}
	if err := os.WriteFile(path, append(buf, '\n'), 0o644); err != nil {
		return fmt.Errorf("write record: %w", err)
	}
	return nil
}

func exitCodeFromError(err error) int {
	if err == nil {
		return 0
	}
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) {
		return exitErr.ExitCode()
	}
	return 1
}
