package ui

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/help"
	"github.com/charmbracelet/bubbles/key"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"

	"vhs-tape-deck/internal/anim"
	"vhs-tape-deck/internal/config"
	"vhs-tape-deck/internal/runner"
)

const (
	tickRate    = 16
	maxLogLines = 2500
)

type tickMsg struct{}

type runEventMsg struct {
	event runner.Event
}

type featureMsg struct {
	info runner.FeatureInfo
}

type model struct {
	cfg      *config.Config
	runner   *runner.Runner
	animator anim.CassetteAnimator

	keys keyMap
	help help.Model

	width  int
	height int

	selected int

	insertedTapeID string
	insertedAtTick int
	appState       anim.State

	runEvents <-chan runner.Event
	runCancel context.CancelFunc
	runningID string

	logs     []string
	viewport viewport.Model

	showHelp       bool
	dryRun         bool
	tickCount      int
	status         string
	lastOutputPath string

	feature runner.FeatureInfo

	tapeStates map[string]anim.State

	styles styles
}

type styles struct {
	shelf      lipgloss.Style
	top        lipgloss.Style
	logs       lipgloss.Style
	footer     lipgloss.Style
	helpBox    lipgloss.Style
	helpBg     lipgloss.Style
	successDot lipgloss.Style
	failedDot  lipgloss.Style
	runDot     lipgloss.Style
	idleDot    lipgloss.Style
	insertDot  lipgloss.Style
	selected   lipgloss.Style
	normal     lipgloss.Style
}

func newStyles() styles {
	return styles{
		shelf:      lipgloss.NewStyle().Border(lipgloss.RoundedBorder()).BorderForeground(lipgloss.Color("62")).Padding(0, 1),
		top:        lipgloss.NewStyle().Border(lipgloss.RoundedBorder()).BorderForeground(lipgloss.Color("69")).Padding(0, 1),
		logs:       lipgloss.NewStyle().Border(lipgloss.RoundedBorder()).BorderForeground(lipgloss.Color("241")).Padding(0, 1),
		footer:     lipgloss.NewStyle().Foreground(lipgloss.Color("249")),
		helpBox:    lipgloss.NewStyle().Border(lipgloss.ThickBorder()).BorderForeground(lipgloss.Color("221")).Background(lipgloss.Color("236")).Padding(1, 2).Width(60),
		helpBg:     lipgloss.NewStyle().Background(lipgloss.Color("236")).Foreground(lipgloss.Color("230")),
		successDot: lipgloss.NewStyle().Foreground(lipgloss.Color("42")),
		failedDot:  lipgloss.NewStyle().Foreground(lipgloss.Color("196")),
		runDot:     lipgloss.NewStyle().Foreground(lipgloss.Color("214")),
		idleDot:    lipgloss.NewStyle().Foreground(lipgloss.Color("245")),
		insertDot:  lipgloss.NewStyle().Foreground(lipgloss.Color("81")),
		selected:   lipgloss.NewStyle().Foreground(lipgloss.Color("230")).Bold(true),
		normal:     lipgloss.NewStyle().Foreground(lipgloss.Color("252")),
	}
}

func NewModel(cfg *config.Config, run *runner.Runner) tea.Model {
	vp := viewport.New(20, 10)
	vp.SetContent("")

	tapeStates := make(map[string]anim.State, len(cfg.Tapes))
	for _, tape := range cfg.Tapes {
		tapeStates[tape.ID] = anim.StateIdle
	}

	hm := help.New()
	hm.ShowAll = false

	return &model{
		cfg:        cfg,
		runner:     run,
		animator:   anim.NewCassetteAnimator(),
		keys:       newKeyMap(),
		help:       hm,
		viewport:   vp,
		appState:   anim.StateIdle,
		status:     "idle",
		tapeStates: tapeStates,
		styles:     newStyles(),
	}
}

func (m *model) Init() tea.Cmd {
	return tea.Batch(nextTick(), detectFeatureCmd(m.runner, m.cfg))
}

func nextTick() tea.Cmd {
	return tea.Tick(time.Second/tickRate, func(time.Time) tea.Msg {
		return tickMsg{}
	})
}

func detectFeatureCmd(run *runner.Runner, cfg *config.Config) tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
		defer cancel()
		return featureMsg{info: run.DetectFeatures(ctx, cfg)}
	}
}

func waitRunEvent(events <-chan runner.Event) tea.Cmd {
	return func() tea.Msg {
		event, ok := <-events
		if !ok {
			return nil
		}
		return runEventMsg{event: event}
	}
}

func (m *model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.resize()

	case tickMsg:
		m.tickCount++
		return m, nextTick()

	case featureMsg:
		m.feature = msg.info
		if msg.info.DetectionFailure != "" {
			m.appendLog(fmt.Sprintf("[feature] %s", msg.info.DetectionFailure))
		}

	case runEventMsg:
		switch msg.event.Type {
		case runner.EventStarted:
			m.appendLog("$ " + msg.event.Message)
			m.status = "running"
		case runner.EventLog:
			m.appendLog(msg.event.Message)
		case runner.EventFinished:
			if msg.event.ExitCode == 0 {
				m.appState = anim.StateSuccess
				if m.runningID != "" {
					m.tapeStates[m.runningID] = anim.StateSuccess
				}
				m.status = "success"
			} else {
				m.appState = anim.StateFailed
				if m.runningID != "" {
					m.tapeStates[m.runningID] = anim.StateFailed
				}
				m.status = fmt.Sprintf("failed (%d)", msg.event.ExitCode)
			}
			if msg.event.Message != "" {
				m.appendLog("[run] " + msg.event.Message)
			}
			if msg.event.Record != nil && len(msg.event.Record.OutputPaths) > 0 {
				m.lastOutputPath = msg.event.Record.OutputPaths[0]
			}
			if msg.event.RecordErr != nil {
				m.appendLog("[record] " + msg.event.RecordErr.Error())
			}
			m.runningID = ""
			m.runEvents = nil
			m.runCancel = nil
		}

		if m.runEvents != nil {
			return m, waitRunEvent(m.runEvents)
		}

	case tea.KeyMsg:
		if key.Matches(msg, m.keys.Quit) {
			if m.runCancel != nil {
				m.runCancel()
			}
			return m, tea.Quit
		}
		if key.Matches(msg, m.keys.Cancel) {
			if m.runCancel != nil {
				m.runCancel()
				m.status = "canceling..."
				m.appendLog("[run] cancel requested")
			}
			return m, nil
		}

		if key.Matches(msg, m.keys.Help) {
			m.showHelp = !m.showHelp
			return m, nil
		}

		if m.showHelp {
			return m, nil
		}

		switch {
		case key.Matches(msg, m.keys.Up):
			if m.selected > 0 {
				m.selected--
			}
		case key.Matches(msg, m.keys.Down):
			if m.selected < len(m.cfg.Tapes)-1 {
				m.selected++
			}
		case key.Matches(msg, m.keys.Insert):
			m.toggleInsert()
		case key.Matches(msg, m.keys.Play):
			return m, m.startRun(runner.ActionPrimary)
		case key.Matches(msg, m.keys.Preview):
			return m, m.startRun(runner.ActionPreview)
		case key.Matches(msg, m.keys.DryRun):
			m.dryRun = !m.dryRun
			m.status = fmt.Sprintf("dry run: %v", m.dryRun)
		case key.Matches(msg, m.keys.Logs):
			m.logs = nil
			m.viewport.SetContent("")
			m.status = "logs cleared"
		}

		m.syncSelectedState()
	}

	return m, nil
}

func (m *model) startRun(action runner.Action) tea.Cmd {
	if m.runEvents != nil {
		m.appendLog("[run] already running")
		return nil
	}
	if m.insertedTapeID == "" {
		m.status = "insert a tape first"
		return nil
	}

	tape, ok := m.findTape(m.insertedTapeID)
	if !ok {
		m.status = "inserted tape is missing"
		return nil
	}
	if action == runner.ActionPreview && !tape.Preview.Enabled {
		m.status = "preview is disabled for this tape"
		return nil
	}
	if action == runner.ActionPreview && m.feature.Checked && !m.feature.HasRenderFrame {
		m.status = "preview unavailable (render-frame not supported)"
		m.appendLog("[preview] Update VCR or set primary_args to an explicit supported subcommand.")
		return nil
	}

	ctx, cancel := context.WithCancel(context.Background())
	events, err := m.runner.Start(ctx, runner.Request{
		Config: m.cfg,
		Tape:   tape,
		Action: action,
		DryRun: m.dryRun,
	})
	if err != nil {
		cancel()
		m.status = "run failed to start"
		m.appendLog("[run] " + err.Error())
		m.appState = anim.StateFailed
		m.tapeStates[tape.ID] = anim.StateFailed
		return nil
	}

	m.runCancel = cancel
	m.runEvents = events
	m.runningID = tape.ID
	m.appState = anim.StateRunning
	m.tapeStates[tape.ID] = anim.StateRunning
	m.status = fmt.Sprintf("running %s", action)
	return waitRunEvent(events)
}

func (m *model) toggleInsert() {
	if len(m.cfg.Tapes) == 0 {
		return
	}
	if m.runEvents != nil {
		m.status = "cannot eject while running"
		return
	}

	tape := m.cfg.Tapes[m.selected]
	if m.insertedTapeID == tape.ID {
		m.insertedTapeID = ""
		m.appState = anim.StateIdle
		m.status = "tape ejected"
		if m.tapeStates[tape.ID] == anim.StateInserted {
			m.tapeStates[tape.ID] = anim.StateIdle
		}
		return
	}

	if m.insertedTapeID != "" {
		m.tapeStates[m.insertedTapeID] = anim.StateIdle
	}
	m.insertedTapeID = tape.ID
	m.insertedAtTick = m.tickCount
	m.appState = anim.StateInserted
	m.tapeStates[tape.ID] = anim.StateInserted
	m.status = "tape inserted"
}

func (m *model) appendLog(line string) {
	line = strings.TrimRight(line, "\n")
	if line == "" {
		return
	}
	m.logs = append(m.logs, line)
	if len(m.logs) > maxLogLines {
		m.logs = m.logs[len(m.logs)-maxLogLines:]
	}
	m.viewport.SetContent(strings.Join(m.logs, "\n"))
	m.viewport.GotoBottom()
}

func (m *model) View() string {
	if m.width == 0 || m.height == 0 {
		return "loading tape deck..."
	}

	if m.showHelp {
		return m.viewHelpOverlay()
	}
	return m.viewMain()
}

func (m *model) viewMain() string {
	leftWidth := m.leftWidth()
	rightWidth := max(30, m.width-leftWidth-1)

	topHeight := max(14, m.height/2)
	bottomHeight := max(6, m.height-topHeight-3)

	shelf := m.styles.shelf.Width(leftWidth - 2).Height(m.height - 4).Render(m.renderShelf(leftWidth - 4))
	top := m.styles.top.Width(rightWidth - 2).Height(topHeight - 2).Render(m.renderTop(rightWidth-4, topHeight-4))
	logs := m.styles.logs.Width(rightWidth - 2).Height(bottomHeight - 2).Render(m.viewport.View())

	right := lipgloss.JoinVertical(lipgloss.Left, top, logs)
	body := lipgloss.JoinHorizontal(lipgloss.Top, shelf, right)

	footer := m.renderFooter()
	return lipgloss.JoinVertical(lipgloss.Left, body, footer)
}

func (m *model) viewHelpOverlay() string {
	hm := m.help
	hm.ShowAll = true
	helpText := "Tape Deck Help\n\n" + hm.View(m.keys) + "\n\nEnter inserts/ejects the selected tape.\nSpace plays the inserted tape.\nCtrl+X cancels an active run.\nP runs preview if enabled."
	box := m.styles.helpBox.Render(helpText)
	return lipgloss.Place(m.width, m.height, lipgloss.Center, lipgloss.Center, box)
}

func (m *model) renderShelf(width int) string {
	var b strings.Builder
	b.WriteString("Tape Shelf\n")
	b.WriteString("---------\n")
	for i, tape := range m.cfg.Tapes {
		marker := " "
		style := m.styles.normal
		if i == m.selected {
			marker = ">"
			style = m.styles.selected
		}

		dot := m.renderDot(m.tapeStates[tape.ID])
		inserted := ""
		if m.insertedTapeID == tape.ID {
			inserted = " [IN]"
		}
		line := fmt.Sprintf("%s %s %s%s", marker, dot, tape.Name, inserted)
		if lipgloss.Width(line) > width {
			line = truncate(line, width)
		}
		b.WriteString(style.Render(line) + "\n")
	}

	if m.feature.Checked {
		rf := "no"
		if m.feature.HasRenderFrame {
			rf = "yes"
		}
		b.WriteString("\nrender-frame: " + rf)
	}

	return b.String()
}

func (m *model) renderTop(width, height int) string {
	tape := m.cfg.Tapes[m.selected]
	tapeState := m.stateForTape(tape.ID)
	inserted := m.insertedTapeID == tape.ID

	animTick := 99
	if inserted {
		animTick = 99
		if diff := m.tickCount - m.insertedAtTick; diff >= 0 && diff < 6 {
			animTick = diff
		}
	}

	cassette := m.animator.Render(
		tape.Name,
		tape.ID,
		animTick,
		tapeState,
		inserted,
		anim.Options{LabelStyle: string(tape.Aesthetic.LabelStyle), ShellColorway: string(tape.Aesthetic.ShellColorway)},
	)

	meta := []string{
		"",
		"Tape Metadata",
		"-------------",
		"Manifest: " + tape.Manifest,
		"Mode: " + string(tape.Mode),
		"Output: " + tape.OutputDir,
		"Primary Args: " + strings.Join(tape.PrimaryArgs, " "),
	}
	if tape.Preview.Enabled {
		meta = append(meta, fmt.Sprintf("Preview: frame=%d args=%s", tape.Preview.Frame, strings.Join(tape.Preview.Args, " ")))
	} else {
		meta = append(meta, "Preview: disabled")
	}
	if tape.Notes != "" {
		meta = append(meta, "Notes: "+tape.Notes)
	}

	left := cassette
	right := strings.Join(meta, "\n")
	joined := lipgloss.JoinHorizontal(lipgloss.Top, left, "  ", right)

	if lipgloss.Height(joined) < height {
		joined += strings.Repeat("\n", height-lipgloss.Height(joined))
	}
	return truncateLines(joined, width)
}

func (m *model) renderFooter() string {
	status := fmt.Sprintf("status=%s | dry-run=%v", m.status, m.dryRun)
	if m.lastOutputPath != "" {
		status += " | last=" + m.lastOutputPath
	}
	keys := m.help.ShortHelpView(m.keys.ShortHelp())
	return m.styles.footer.Render(keys + "\n" + status)
}

func (m *model) resize() {
	leftWidth := m.leftWidth()
	rightWidth := max(30, m.width-leftWidth-1)
	topHeight := max(14, m.height/2)
	bottomHeight := max(6, m.height-topHeight-3)

	m.viewport.Width = max(10, rightWidth-6)
	m.viewport.Height = max(3, bottomHeight-4)
	m.viewport.SetContent(strings.Join(m.logs, "\n"))
	m.viewport.GotoBottom()
}

func (m *model) leftWidth() int {
	return max(28, min(38, m.width/3))
}

func (m *model) stateForTape(tapeID string) anim.State {
	if m.insertedTapeID == tapeID {
		return m.appState
	}
	state := m.tapeStates[tapeID]
	if state == "" {
		return anim.StateIdle
	}
	return state
}

func (m *model) syncSelectedState() {
	tape := m.cfg.Tapes[m.selected]
	if m.insertedTapeID != tape.ID {
		return
	}
	if m.appState == "" {
		m.appState = anim.StateInserted
	}
}

func (m *model) renderDot(state anim.State) string {
	switch state {
	case anim.StateRunning:
		return m.styles.runDot.Render("●")
	case anim.StateInserted:
		return m.styles.insertDot.Render("●")
	case anim.StateSuccess:
		return m.styles.successDot.Render("●")
	case anim.StateFailed:
		return m.styles.failedDot.Render("●")
	default:
		return m.styles.idleDot.Render("●")
	}
}

func (m *model) findTape(id string) (config.Tape, bool) {
	for _, tape := range m.cfg.Tapes {
		if tape.ID == id {
			return tape, true
		}
	}
	return config.Tape{}, false
}

func truncate(s string, width int) string {
	if width <= 0 {
		return ""
	}
	r := []rune(s)
	if len(r) <= width {
		return s
	}
	if width <= 1 {
		return string(r[:width])
	}
	return string(r[:width-1]) + "…"
}

func truncateLines(v string, width int) string {
	if width <= 0 {
		return ""
	}
	lines := strings.Split(v, "\n")
	for i := range lines {
		lines[i] = truncate(lines[i], width)
	}
	return strings.Join(lines, "\n")
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
