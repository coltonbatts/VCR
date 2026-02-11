package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/progress"
	"github.com/charmbracelet/bubbles/spinner"
	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/coltonbatts/vcr/tui/internal/db"
)

// Aesthetic Constants (Editorial Modernism / Brutalist)
var (
	headerStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FFFFFF")).
			Background(lipgloss.Color("#000000")).
			Padding(1, 4).
			BorderStyle(lipgloss.ThickBorder()).
			BorderForeground(lipgloss.Color("#FFFFFF"))

	statusStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#888888")).
			Italic(true)

	borderStyle = lipgloss.NewStyle().
			BorderStyle(lipgloss.NormalBorder()).
			BorderForeground(lipgloss.Color("#FFFFFF"))
)

type IPCMessage struct {
	Type    string  `json:"type"`
	Percent float64 `json:"percent,omitempty"`
	Status  string  `json:"status,omitempty"`
	Path    string  `json:"path,omitempty"`
	Message string  `json:"message,omitempty"`
	Code    int     `json:"code,omitempty"`
}

type model struct {
	initializing bool
	handshaking  bool
	running      bool
	spinner      spinner.Model
	progress     progress.Model
	textInput    textinput.Model
	status       string
	gpuInfo      string
	llmStatus    string
	skillStatus  string
	err          error
}

type gpuScanMsg string
type llmScanMsg string
type handshakeMsg struct {
	success bool
	details string
}
type skillDoneMsg struct{}
type errMsg error

func initialModel() model {
	s := spinner.New()
	s.Spinner = spinner.Line
	s.Style = lipgloss.NewStyle().Foreground(lipgloss.Color("#FFFFFF"))

	p := progress.New(progress.WithDefaultGradient())
	p.Width = 40

	ti := textinput.New()
	ti.Placeholder = "Enter Agentic Prompt..."
	ti.Focus()

	return model{
		initializing: true,
		spinner:      s,
		progress:     p,
		textInput:    ti,
		status:       "INITIALIZING VCR SYSTEM...",
		gpuInfo:      "SCANNING GPU...",
		llmStatus:    "SCANNING LOCAL LLMs...",
	}
}

func (m model) Init() tea.Cmd {
	return tea.Batch(
		m.spinner.Tick,
		m.scanGPU,
		m.scanLLM,
	)
}

func (m model) scanGPU() tea.Msg {
	out, _ := exec.Command("vcr", "doctor").Output()
	s := string(out)
	if strings.Contains(s, "Backend: OK") {
		return gpuScanMsg("GPU: ACCELERATION ACTIVE")
	}
	return gpuScanMsg("GPU: SOFTWARE RENDERING")
}

func (m model) scanLLM() tea.Msg {
	client := http.Client{Timeout: 2 * time.Second}

	// 1. Check LM Studio (Priority)
	if resp, err := client.Get("http://127.0.0.1:1234/v1/models"); err == nil && resp.StatusCode == 200 {
		var mData struct {
			Data []struct {
				ID string `json:"id"`
			} `json:"data"`
		}
		if body, rErr := io.ReadAll(resp.Body); rErr == nil {
			json.Unmarshal(body, &mData)
			if len(mData.Data) > 0 {
				return llmScanMsg("STUDIO:" + mData.Data[0].ID)
			}
		}
		return llmScanMsg("LM_STUDIO")
	}

	// 2. Check Ollama
	if resp, err := client.Get("http://localhost:11434/api/tags"); err == nil && resp.StatusCode == 200 {
		return llmScanMsg("OLLAMA")
	}

	return llmScanMsg("NONE")
}

func (m model) handshakeLLM(modelID string) tea.Cmd {
	return func() tea.Msg {
		client := http.Client{Timeout: 10 * time.Second}

		// If modelID is generic, try to use "local-model" or just any
		id := modelID
		if id == "" {
			id = "local-model"
		}

		payload, _ := json.Marshal(map[string]interface{}{
			"model": id,
			"messages": []map[string]string{
				{"role": "user", "content": "ping"},
			},
			"max_tokens": 1,
		})

		resp, err := client.Post("http://127.0.0.1:1234/v1/chat/completions", "application/json", strings.NewReader(string(payload)))
		if err != nil {
			return handshakeMsg{success: false, details: "Timeout or Connection Refused"}
		}
		defer resp.Body.Close()

		if resp.StatusCode != 200 {
			return handshakeMsg{success: false, details: fmt.Sprintf("HTTP %d", resp.StatusCode)}
		}

		return handshakeMsg{success: true, details: "Local Brain Verified"}
	}
}

func (m model) runSkill(prompt string) tea.Cmd {
	return func() tea.Msg {
		// In a real app, we'd pick the skill based on the prompt
		cmd := exec.Command("go", "run", "skills/video-gen/main.go")
		stdout, _ := cmd.StdoutPipe()
		cmd.Start()

		scanner := bufio.NewScanner(stdout)
		for scanner.Scan() {
			var msg IPCMessage
			if err := json.Unmarshal(scanner.Bytes(), &msg); err == nil {
				// We need a way to send this back to the main loop
				// Since we're in a tea.Cmd, we can't easily emit multiple msgs
				// but for this MVP we'll just handle the last one or
				// better yet, use a channel (omitted for simplicity here)
			}
		}
		cmd.Wait()
		return skillDoneMsg{}
	}
}

type skillUpdateMsg struct {
	msg     IPCMessage
	scanner *bufio.Scanner
}

// Better approach for IPC streaming in Bubble Tea:
func listenToSkill(scanner *bufio.Scanner) tea.Cmd {
	return func() tea.Msg {
		if scanner.Scan() {
			var msg IPCMessage
			if err := json.Unmarshal(scanner.Bytes(), &msg); err == nil {
				return skillUpdateMsg{msg: msg, scanner: scanner}
			}
			// If JSON parse fails, try next line
			return listenToSkill(scanner)()
		}
		return skillDoneMsg{}
	}
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmd tea.Cmd

	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "ctrl+c", "q":
			return m, tea.Quit
		case "enter":
			if !m.initializing && !m.running {
				prompt := m.textInput.Value()
				if prompt == "" {
					return m, nil
				}
				m.running = true
				m.skillStatus = "Launching Agentic Skill..."
				cmd := exec.Command("go", "run", "skills/video-gen/main.go", prompt)
				stdout, _ := cmd.StdoutPipe()
				cmd.Start()
				m.textInput.Reset()
				m.textInput.Blur()
				return m, listenToSkill(bufio.NewScanner(stdout))
			}
		}
	case spinner.TickMsg:
		m.spinner, cmd = m.spinner.Update(msg)
		return m, cmd
	case gpuScanMsg:
		m.gpuInfo = string(msg)
		return m, nil
	case llmScanMsg:
		s := string(msg)
		if strings.HasPrefix(s, "STUDIO:") {
			modelID := strings.TrimPrefix(s, "STUDIO:")
			m.llmStatus = "LLM: LM STUDIO (" + modelID + ")"
			m.handshaking = true
			m.status = "HANDSHAKING WITH BRAIN..."
			return m, m.handshakeLLM(modelID)
		}
		if s == "LM_STUDIO" {
			m.llmStatus = "LLM: LM STUDIO DETECTED"
			m.handshaking = true
			m.status = "HANDSHAKING WITH BRAIN..."
			return m, m.handshakeLLM("local-model")
		}
		m.llmStatus = "LLM: " + s
		m.initializing = false
		m.status = "VCR READY"
		return m, nil
	case handshakeMsg:
		m.handshaking = false
		m.initializing = false
		if msg.success {
			m.llmStatus = "LLM: LOCAL BRAIN VERIFIED"
			m.status = "VCR READY"
		} else {
			m.llmStatus = "LLM: HANDSHAKE FAILED (" + msg.details + ")"
			m.status = "VCR READY (OFFLINE)"
		}
		return m, nil
	case skillUpdateMsg:
		if msg.msg.Type == "error" {
			m.skillStatus = "ERROR: " + msg.msg.Message
			return m, nil // Stop listening on error
		}
		m.skillStatus = msg.msg.Status
		if msg.msg.Type == "progress" {
			progCmd := m.progress.SetPercent(msg.msg.Percent)
			return m, tea.Batch(progCmd, listenToSkill(msg.scanner))
		}
		if msg.msg.Type == "artifact" {
			m.skillStatus = "SUCCESS: " + msg.msg.Path
		}
		return m, listenToSkill(msg.scanner)
	case skillDoneMsg:
		m.running = false
		m.status = "VCR READY"
		m.textInput.Focus()
		return m, nil
	case progress.FrameMsg:
		newModel, cmd := m.progress.Update(msg)
		m.progress = newModel.(progress.Model)
		return m, cmd
	}

	m.textInput, cmd = m.textInput.Update(msg)
	return m, cmd
}

func (m model) View() string {
	header := headerStyle.Render("VCR // HUB")

	var statusLine string
	if m.initializing {
		statusLine = fmt.Sprintf("%s %s", m.spinner.View(), m.status)
	} else if m.handshaking {
		statusLine = fmt.Sprintf("%s %s", m.spinner.View(), m.status)
	} else {
		statusLine = fmt.Sprintf("✓ %s", m.status)
	}

	panels := []string{
		statusLine,
		statusStyle.Render(m.gpuInfo),
		statusStyle.Render(m.llmStatus),
	}

	if m.running {
		// Agentic Row
		agentStatus := lipgloss.NewStyle().
			Width(56).
			Foreground(lipgloss.Color("#FFFFFF")).
			Render(m.skillStatus)

		panels = append(panels, "\n[AGENTIC ENGINE ACTIVE]", agentStatus, m.progress.View())
	}

	content := borderStyle.Width(60).Padding(1).Render(lipgloss.JoinVertical(lipgloss.Left, panels...))

	footer := "\n"
	if !m.initializing && !m.running {
		footer += lipgloss.NewStyle().
			Background(lipgloss.Color("#FFFFFF")).
			Foreground(lipgloss.Color("#000000")).
			Padding(0, 1).
			Render(" PROMPT ")
		footer += " " + m.textInput.View()
	}
	footer += "\n\n [q] quit | [ctrl+c] terminate"

	return fmt.Sprintf("\n%s\n\n%s\n%s", header, content, footer)
}

func main() {
	// Auto-Init DB
	database, err := db.Open()
	if err == nil {
		// Initialize with embedded schema if we had one,
		// but for now we'll just seed if it's the first run
		database.SeedMockData()
		database.Conn.Close()
	}

	p := tea.NewProgram(initialModel())
	if _, err := p.Run(); err != nil {
		fmt.Printf("Error: %v", err)
		os.Exit(1)
	}
}
