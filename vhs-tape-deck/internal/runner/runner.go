package runner

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"vhs-tape-deck/internal/config"
)

type Action string

const (
	ActionPrimary Action = "primary"
	ActionPreview Action = "preview"
)

type EventType string

const (
	EventStarted  EventType = "started"
	EventLog      EventType = "log"
	EventFinished EventType = "finished"
)

type Event struct {
	Type      EventType
	Message   string
	Record    *RunRecord
	Plan      *CommandPlan
	ExitCode  int
	RecordErr error
}

type Request struct {
	Config *config.Config
	Tape   config.Tape
	Action Action
	DryRun bool
}

type FeatureInfo struct {
	Checked          bool
	HasRenderFrame   bool
	HelpSnippet      string
	DetectionFailure string
}

type CommandPlan struct {
	RunID        string
	Timestamp    time.Time
	Binary       string
	Args         []string
	CWD          string
	EnvOverrides map[string]string
	ManifestPath string
	OutputDir    string
	OutputPaths  []string
	Action       Action
	DryRun       bool
	RecordPath   string
}

type Runner struct {
	nowFn func() time.Time

	mu       sync.Mutex
	counter  map[string]int
	feature  FeatureInfo
	checked  bool
	checkErr string
}

func New(nowFn func() time.Time) *Runner {
	if nowFn == nil {
		nowFn = time.Now
	}
	return &Runner{
		nowFn:   nowFn,
		counter: map[string]int{},
	}
}

func (r *Runner) DetectFeatures(ctx context.Context, cfg *config.Config) FeatureInfo {
	r.mu.Lock()
	if r.checked {
		defer r.mu.Unlock()
		return r.feature
	}
	r.mu.Unlock()

	helpCtx, cancel := context.WithTimeout(ctx, 3*time.Second)
	defer cancel()

	cmd := exec.CommandContext(helpCtx, cfg.VCRBinary, "--help")
	cmd.Dir = cfg.ProjectRoot
	out, err := cmd.CombinedOutput()
	help := string(out)

	feature := FeatureInfo{Checked: true, HasRenderFrame: strings.Contains(help, "render-frame")}
	if len(help) > 220 {
		feature.HelpSnippet = strings.TrimSpace(help[:220])
	} else {
		feature.HelpSnippet = strings.TrimSpace(help)
	}
	if err != nil {
		feature.DetectionFailure = err.Error()
	}

	r.mu.Lock()
	r.feature = feature
	r.checked = true
	r.mu.Unlock()

	return feature
}

func (r *Runner) Start(ctx context.Context, req Request) (<-chan Event, error) {
	plan, record, err := r.BuildPlan(req)
	if err != nil {
		return nil, err
	}

	events := make(chan Event, 128)
	go r.execute(ctx, plan, record, events)
	return events, nil
}

func (r *Runner) BuildPlan(req Request) (*CommandPlan, *RunRecord, error) {
	if req.Config == nil {
		return nil, nil, errors.New("missing config")
	}
	if strings.TrimSpace(req.Tape.ID) == "" {
		return nil, nil, errors.New("missing tape")
	}
	if req.Action == ActionPreview && !req.Tape.Preview.Enabled {
		return nil, nil, fmt.Errorf("tape %q has no preview configured", req.Tape.ID)
	}

	ts := r.nowFn()
	runID := r.nextRunID(req.Tape.ID, ts)

	manifestPath, err := config.ResolveManifestPath(req.Config.ProjectRoot, req.Tape.Manifest)
	if err != nil {
		return nil, nil, fmt.Errorf("resolve manifest path: %w", err)
	}
	outputDir, err := config.ResolvePath(req.Tape.OutputDir, req.Config.ProjectRoot)
	if err != nil {
		return nil, nil, fmt.Errorf("resolve output dir: %w", err)
	}

	args, outputPaths, err := buildArgs(req.Tape, req.Action, manifestPath, outputDir, runID, req.Config.OutputFlag)
	if err != nil {
		return nil, nil, err
	}

	recordPath := filepath.Join(req.Config.RunsDir, "records", runID+".json")
	plan := &CommandPlan{
		RunID:        runID,
		Timestamp:    ts,
		Binary:       req.Config.VCRBinary,
		Args:         args,
		CWD:          req.Config.ProjectRoot,
		EnvOverrides: cloneMap(req.Config.Env),
		ManifestPath: manifestPath,
		OutputDir:    outputDir,
		OutputPaths:  outputPaths,
		Action:       req.Action,
		DryRun:       req.DryRun,
		RecordPath:   recordPath,
	}

	record := &RunRecord{
		Timestamp:    ts,
		RunID:        runID,
		TapeID:       req.Tape.ID,
		TapeName:     req.Tape.Name,
		ManifestPath: manifestPath,
		Command:      append([]string{req.Config.VCRBinary}, args...),
		CWD:          req.Config.ProjectRoot,
		EnvOverrides: cloneMap(req.Config.Env),
		ExitCode:     -1,
		OutputPaths:  append([]string(nil), outputPaths...),
		Action:       req.Action,
		DryRun:       req.DryRun,
	}

	return plan, record, nil
}

func buildArgs(tape config.Tape, action Action, manifestPath, outputDir, runID, outputFlag string) ([]string, []string, error) {
	var args []string
	var extra []string
	outputFlag = strings.TrimSpace(outputFlag)
	if outputFlag == "" {
		outputFlag = "--output"
	}

	switch action {
	case ActionPrimary:
		extra = append(extra, tape.PrimaryArgs...)
		if hasSubcommand(extra) {
			args = append(args, extra...)
		} else {
			if tape.Mode == config.ModeFrame {
				args = append(args, "render-frame", manifestPath)
				args = append(args, extra...)
				if !hasFrameFlag(args) {
					args = append(args, "--frame", "0")
				}
			} else {
				args = append(args, "render", manifestPath)
				args = append(args, extra...)
			}
		}
	case ActionPreview:
		extra = append(extra, tape.Preview.Args...)
		if hasSubcommand(extra) {
			args = append(args, extra...)
		} else {
			args = append(args, "render-frame", manifestPath)
			args = append(args, extra...)
			frame := tape.Preview.Frame
			if frame < 0 {
				frame = 0
			}
			if !hasFrameFlag(args) {
				args = append(args, "--frame", strconv.Itoa(frame))
			}
		}
	default:
		return nil, nil, fmt.Errorf("unsupported action %q", action)
	}

	outputPaths := []string{}
	if !hasOutputFlag(args, outputFlag) {
		ext := ".mov"
		if tape.Mode == config.ModeFrame || action == ActionPreview {
			ext = ".png"
		}
		suffix := ""
		if action == ActionPreview {
			suffix = "_preview"
		}
		outputPath := filepath.Join(outputDir, runID+suffix+ext)
		args = append(args, outputFlag, outputPath)
		outputPaths = append(outputPaths, outputPath)
	}

	return args, outputPaths, nil
}

func hasSubcommand(args []string) bool {
	if len(args) == 0 {
		return false
	}
	first := strings.TrimSpace(args[0])
	if first == "" {
		return false
	}
	return !strings.HasPrefix(first, "-")
}

func hasFrameFlag(args []string) bool {
	for i, arg := range args {
		if arg == "--frame" {
			if i+1 < len(args) {
				return true
			}
			return true
		}
		if strings.HasPrefix(arg, "--frame=") {
			return true
		}
	}
	return false
}

func hasOutputFlag(args []string, outputFlag string) bool {
	flag := strings.TrimSpace(outputFlag)
	if flag == "" {
		flag = "--output"
	}

	aliases := []string{flag}
	if flag == "--output" {
		aliases = append(aliases, "-o")
	}

	for _, arg := range args {
		for _, alias := range aliases {
			if arg == alias {
				return true
			}
			if strings.HasPrefix(arg, alias+"=") {
				return true
			}
		}
	}
	return false
}

func (r *Runner) execute(ctx context.Context, plan *CommandPlan, record *RunRecord, events chan<- Event) {
	defer close(events)

	events <- Event{Type: EventStarted, Message: shellQuote(append([]string{plan.Binary}, plan.Args...)...), Plan: plan, Record: record}

	if err := os.MkdirAll(plan.OutputDir, 0o755); err != nil {
		record.ExitCode = 1
		recordErr := WriteRunRecord(plan.RecordPath, record)
		events <- Event{Type: EventFinished, Message: fmt.Sprintf("create output dir: %v", err), ExitCode: record.ExitCode, Record: record, RecordErr: recordErr}
		return
	}
	if err := os.MkdirAll(filepath.Dir(plan.RecordPath), 0o755); err != nil {
		record.ExitCode = 1
		recordErr := WriteRunRecord(plan.RecordPath, record)
		events <- Event{Type: EventFinished, Message: fmt.Sprintf("create record dir: %v", err), ExitCode: record.ExitCode, Record: record, RecordErr: recordErr}
		return
	}

	if plan.DryRun {
		record.ExitCode = 0
		recordErr := WriteRunRecord(plan.RecordPath, record)
		events <- Event{Type: EventLog, Message: "[dry-run] command not executed"}
		events <- Event{Type: EventFinished, Message: "dry run complete", ExitCode: 0, Record: record, RecordErr: recordErr}
		return
	}

	cmd := exec.CommandContext(ctx, plan.Binary, plan.Args...)
	cmd.Dir = plan.CWD
	cmd.Env = mergeEnv(os.Environ(), plan.EnvOverrides)

	stdout, err := cmd.StdoutPipe()
	if err != nil {
		record.ExitCode = 1
		recordErr := WriteRunRecord(plan.RecordPath, record)
		events <- Event{Type: EventFinished, Message: fmt.Sprintf("stdout pipe: %v", err), ExitCode: 1, Record: record, RecordErr: recordErr}
		return
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		record.ExitCode = 1
		recordErr := WriteRunRecord(plan.RecordPath, record)
		events <- Event{Type: EventFinished, Message: fmt.Sprintf("stderr pipe: %v", err), ExitCode: 1, Record: record, RecordErr: recordErr}
		return
	}

	if err := cmd.Start(); err != nil {
		record.ExitCode = exitCodeFromError(err)
		recordErr := WriteRunRecord(plan.RecordPath, record)
		events <- Event{Type: EventFinished, Message: fmt.Sprintf("start command: %v", err), ExitCode: record.ExitCode, Record: record, RecordErr: recordErr}
		return
	}

	var wg sync.WaitGroup
	wg.Add(2)
	go func() {
		defer wg.Done()
		scanPipe("out", stdout, events)
	}()
	go func() {
		defer wg.Done()
		scanPipe("err", stderr, events)
	}()

	waitErr := cmd.Wait()
	wg.Wait()

	exitCode := exitCodeFromError(waitErr)
	record.ExitCode = exitCode
	recordErr := WriteRunRecord(plan.RecordPath, record)

	msg := "run complete"
	if waitErr != nil {
		if errors.Is(ctx.Err(), context.Canceled) {
			msg = "run canceled"
		} else {
			msg = waitErr.Error()
		}
	}

	events <- Event{Type: EventFinished, Message: msg, ExitCode: exitCode, Record: record, RecordErr: recordErr}
}

func scanPipe(stream string, r io.Reader, events chan<- Event) {
	scanner := bufio.NewScanner(r)
	buf := make([]byte, 0, 64*1024)
	scanner.Buffer(buf, 1024*1024)
	for scanner.Scan() {
		events <- Event{Type: EventLog, Message: fmt.Sprintf("[%s] %s", stream, scanner.Text())}
	}
	if err := scanner.Err(); err != nil {
		events <- Event{Type: EventLog, Message: fmt.Sprintf("[%s] scan error: %v", stream, err)}
	}
}

func (r *Runner) nextRunID(tapeID string, ts time.Time) string {
	r.mu.Lock()
	defer r.mu.Unlock()

	r.counter[tapeID]++
	counter := r.counter[tapeID]
	tsPart := ts.Format("20060102_150405")
	return fmt.Sprintf("%s_%s_%03d", tsPart, sanitizeID(tapeID), counter)
}

func sanitizeID(v string) string {
	v = strings.TrimSpace(v)
	if v == "" {
		return "tape"
	}
	replacer := strings.NewReplacer(" ", "-", "/", "-", "\\", "-", ":", "-")
	v = replacer.Replace(v)
	return v
}

func shellQuote(parts ...string) string {
	q := make([]string, 0, len(parts))
	for _, p := range parts {
		if p == "" {
			q = append(q, "''")
			continue
		}
		if strings.IndexFunc(p, func(r rune) bool {
			return r == ' ' || r == '\t' || r == '\n' || r == '\'' || r == '"'
		}) >= 0 {
			replaced := strings.ReplaceAll(p, "'", "'\\''")
			q = append(q, "'"+replaced+"'")
			continue
		}
		q = append(q, p)
	}
	return strings.Join(q, " ")
}

func mergeEnv(base []string, overrides map[string]string) []string {
	if len(overrides) == 0 {
		return base
	}
	kv := make(map[string]string, len(base)+len(overrides))
	for _, entry := range base {
		if idx := strings.Index(entry, "="); idx >= 0 {
			kv[entry[:idx]] = entry[idx+1:]
		}
	}
	for key, value := range overrides {
		kv[key] = value
	}

	out := make([]string, 0, len(kv))
	for key, value := range kv {
		out = append(out, key+"="+value)
	}
	return out
}

func cloneMap(in map[string]string) map[string]string {
	if len(in) == 0 {
		return map[string]string{}
	}
	out := make(map[string]string, len(in))
	for k, v := range in {
		out[k] = v
	}
	return out
}
