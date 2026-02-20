package runner

import (
	"path/filepath"
	"strings"
	"testing"
	"time"

	"vhs-tape-deck/internal/config"
)

func TestBuildPlanPrimaryDefaults(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	r := New(func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) })

	plan, record, err := r.BuildPlan(Request{Config: cfg, Tape: cfg.Tapes[0], Action: ActionPrimary, DryRun: false})
	if err != nil {
		t.Fatalf("BuildPlan: %v", err)
	}

	if plan.RunID != "20260220_123001_alpha_001" {
		t.Fatalf("unexpected run id: %s", plan.RunID)
	}
	if plan.Args[0] != "render" {
		t.Fatalf("expected render command, got %q", plan.Args[0])
	}
	if !contains(plan.Args, "--output") {
		t.Fatalf("expected output flag in args: %v", plan.Args)
	}
	if len(record.Command) == 0 || record.Command[0] != "vcr" {
		t.Fatalf("unexpected record command: %v", record.Command)
	}
	if got := record.ManifestPath; !strings.HasSuffix(got, filepath.FromSlash("project/manifests/alpha.yaml")) {
		t.Fatalf("unexpected manifest path: %s", got)
	}
}

func TestBuildPlanSupportsFullPrimaryArgs(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	cfg.Tapes[0].PrimaryArgs = []string{"render", "./manifests/custom.yaml", "--quality", "draft", "--output", "./out.mov"}

	r := New(func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) })
	plan, _, err := r.BuildPlan(Request{Config: cfg, Tape: cfg.Tapes[0], Action: ActionPrimary})
	if err != nil {
		t.Fatalf("BuildPlan: %v", err)
	}

	if plan.Args[0] != "render" {
		t.Fatalf("expected first arg to be full subcommand: %v", plan.Args)
	}
	if !contains(plan.Args, "./manifests/custom.yaml") {
		t.Fatalf("expected custom manifest arg in plan: %v", plan.Args)
	}
	if contains(plan.Args, filepath.Join(cfg.ProjectRoot, "manifests", "alpha.yaml")) {
		t.Fatalf("did not expect default manifest to be injected: %v", plan.Args)
	}
}

func TestBuildPlanPreviewDefaults(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	tape := cfg.Tapes[1]
	if tape.Mode != config.ModeFrame {
		t.Fatalf("expected frame tape")
	}

	r := New(func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) })
	plan, _, err := r.BuildPlan(Request{Config: cfg, Tape: tape, Action: ActionPreview})
	if err != nil {
		t.Fatalf("BuildPlan: %v", err)
	}

	joined := strings.Join(plan.Args, " ")
	if !strings.Contains(joined, "render-frame") || !strings.Contains(joined, "--frame 42") {
		t.Fatalf("unexpected preview args: %v", plan.Args)
	}
	if !strings.HasSuffix(plan.OutputPaths[0], "_preview.png") {
		t.Fatalf("unexpected preview output: %v", plan.OutputPaths)
	}
}

func TestBuildPlanPreviewDoesNotDuplicateFrameFlag(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	tape := cfg.Tapes[1]
	tape.Preview.Args = []string{"--frame", "99"}

	r := New(func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) })
	plan, _, err := r.BuildPlan(Request{Config: cfg, Tape: tape, Action: ActionPreview})
	if err != nil {
		t.Fatalf("BuildPlan: %v", err)
	}

	if countFrameSpecs(plan.Args) != 1 {
		t.Fatalf("expected exactly one frame flag for preview args: %v", plan.Args)
	}
	if strings.Contains(strings.Join(plan.Args, " "), "--frame 42") {
		t.Fatalf("unexpected default preview frame injected with explicit frame flag: %v", plan.Args)
	}
}

func TestBuildPlanPrimaryFrameDoesNotDuplicateFrameFlag(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	tape := cfg.Tapes[1]
	tape.PrimaryArgs = []string{"--frame", "160"}

	r := New(func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) })
	plan, _, err := r.BuildPlan(Request{Config: cfg, Tape: tape, Action: ActionPrimary})
	if err != nil {
		t.Fatalf("BuildPlan: %v", err)
	}

	if countFrameSpecs(plan.Args) != 1 {
		t.Fatalf("expected exactly one frame flag, got args: %v", plan.Args)
	}
	joined := strings.Join(plan.Args, " ")
	if strings.Contains(joined, "--frame 0") {
		t.Fatalf("unexpected injected default frame in args: %v", plan.Args)
	}
}

func TestBuildPlanUsesConfiguredOutputFlag(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	cfg.OutputFlag = "--out"

	r := New(func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) })
	plan, _, err := r.BuildPlan(Request{Config: cfg, Tape: cfg.Tapes[0], Action: ActionPrimary})
	if err != nil {
		t.Fatalf("BuildPlan: %v", err)
	}

	if !contains(plan.Args, "--out") {
		t.Fatalf("expected configured output flag in args: %v", plan.Args)
	}
	if contains(plan.Args, "--output") {
		t.Fatalf("did not expect default output flag when custom is configured: %v", plan.Args)
	}
}

func TestBuildPlanHonorsCustomOutputEqualsSyntax(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	cfg.OutputFlag = "--out"
	cfg.Tapes[0].PrimaryArgs = []string{"render", "./manifests/custom.yaml", "--out=./already.mov"}

	r := New(func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) })
	plan, _, err := r.BuildPlan(Request{Config: cfg, Tape: cfg.Tapes[0], Action: ActionPrimary})
	if err != nil {
		t.Fatalf("BuildPlan: %v", err)
	}

	if countMatches(plan.Args, "--out") != 0 {
		t.Fatalf("did not expect injected separate --out flag when --out= is present: %v", plan.Args)
	}
}

func TestBuildPlanRunIDCounterIncrements(t *testing.T) {
	t.Parallel()

	cfg := testConfig(t)
	now := func() time.Time { return time.Date(2026, 2, 20, 12, 30, 1, 0, time.UTC) }
	r := New(now)

	first, _, err := r.BuildPlan(Request{Config: cfg, Tape: cfg.Tapes[0], Action: ActionPrimary})
	if err != nil {
		t.Fatalf("BuildPlan first: %v", err)
	}
	second, _, err := r.BuildPlan(Request{Config: cfg, Tape: cfg.Tapes[0], Action: ActionPrimary})
	if err != nil {
		t.Fatalf("BuildPlan second: %v", err)
	}
	if first.RunID == second.RunID {
		t.Fatalf("expected unique run IDs, got %s", first.RunID)
	}
	if !strings.HasSuffix(second.RunID, "_002") {
		t.Fatalf("expected counter suffix _002, got %s", second.RunID)
	}
}

func testConfig(t *testing.T) *config.Config {
	t.Helper()
	tmp := t.TempDir()
	cfg := &config.Config{
		VCRBinary:   "vcr",
		ProjectRoot: filepath.Join(tmp, "project"),
		RunsDir:     filepath.Join(tmp, "runs"),
		Env:         map[string]string{"VCR_SEED": "0"},
		Tapes: []config.Tape{
			{
				ID:       "alpha",
				Name:     "Alpha",
				Manifest: "./manifests/alpha.yaml",
				Mode:     config.ModeVideo,
				Preview:  config.Preview{Enabled: true, Frame: 8},
			},
			{
				ID:       "still",
				Name:     "Still",
				Manifest: "./manifests/still.yaml",
				Mode:     config.ModeFrame,
				Preview:  config.Preview{Enabled: true, Frame: 42},
			},
		},
	}
	if err := config.ApplyDefaults(cfg, filepath.Join(tmp, "config.yaml"), cfg.ProjectRoot); err != nil {
		t.Fatalf("ApplyDefaults: %v", err)
	}
	return cfg
}

func contains(items []string, target string) bool {
	for _, item := range items {
		if item == target {
			return true
		}
	}
	return false
}

func countFrameSpecs(args []string) int {
	count := 0
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if arg == "--frame" {
			count++
			continue
		}
		if strings.HasPrefix(arg, "--frame=") {
			count++
		}
	}
	return count
}

func countMatches(items []string, target string) int {
	count := 0
	for _, item := range items {
		if item == target {
			count++
		}
	}
	return count
}
