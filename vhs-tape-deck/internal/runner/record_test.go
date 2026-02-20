package runner

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestWriteRunRecord(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	recordPath := filepath.Join(tmp, "records", "run.json")

	record := &RunRecord{
		Timestamp:    time.Date(2026, 2, 20, 12, 30, 0, 0, time.UTC),
		RunID:        "20260220_123000_alpha_001",
		TapeID:       "alpha",
		TapeName:     "Alpha",
		ManifestPath: "/tmp/project/manifests/alpha.yaml",
		Command:      []string{"vcr", "render", "/tmp/project/manifests/alpha.yaml"},
		CWD:          "/tmp/project",
		EnvOverrides: map[string]string{"VCR_SEED": "0"},
		ExitCode:     0,
		OutputPaths:  []string{"/tmp/runs/alpha/out.mov"},
		Action:       ActionPrimary,
		DryRun:       false,
	}

	if err := WriteRunRecord(recordPath, record); err != nil {
		t.Fatalf("WriteRunRecord: %v", err)
	}

	buf, err := os.ReadFile(recordPath)
	if err != nil {
		t.Fatalf("read record: %v", err)
	}

	var decoded RunRecord
	if err := json.Unmarshal(buf, &decoded); err != nil {
		t.Fatalf("unmarshal record: %v", err)
	}
	if decoded.RunID != record.RunID {
		t.Fatalf("expected run id %s got %s", record.RunID, decoded.RunID)
	}
	if decoded.ExitCode != 0 {
		t.Fatalf("unexpected exit code: %d", decoded.ExitCode)
	}
}
