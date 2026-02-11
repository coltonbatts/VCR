package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"regexp"
	"strings"

	"github.com/coltonbatts/vcr/tui/internal/db"
)

type IPCMessage struct {
	Type    string  `json:"type"`
	Percent float64 `json:"percent,omitempty"`
	Status  string  `json:"status,omitempty"`
	Path    string  `json:"path,omitempty"`
}

func emit(msg IPCMessage) {
	b, _ := json.Marshal(msg)
	fmt.Println(string(b))
}

func main() {
	if len(os.Args) < 2 {
		emit(IPCMessage{Type: "status", Status: "Error: No prompt provided."})
		os.Exit(1)
	}
	prompt := os.Args[1]

	// 1. Fetch Context from SQLite
	emit(IPCMessage{Type: "status", Status: "Retrieving creative context from Intelligence Tree..."})
	database, err := db.Open()
	if err != nil {
		emit(IPCMessage{Type: "status", Status: fmt.Sprintf("Error opening DB: %v", err)})
		os.Exit(1)
	}
	defer database.Conn.Close()

	nodes, err := database.GetContextNodes()
	contextStr := ""
	if err == nil {
		contextStr = strings.Join(nodes, "\n")
	}

	// 2. Query LM Studio (localhost:1234)
	emit(IPCMessage{Type: "status", Status: "Thinking... (Querying LM Studio at 1234/v1)"})

	messaage := fmt.Sprintf(`You are VCR, an expert motion graphics agent. 
Generate a valid VCR manifest YAML based on this prompt: "%s"

Creative Context from other tools:
%s

IMPORTANT: Output ONLY the valid YAML manifest. No talk. No markdown blocks. Just the YAML.
A VCR manifest must have:
version: 1
environment:
  resolution: {width: 1280, height: 720}
  fps: 24
  duration: 5.0
layers:
  - id: background
    procedural: {type: solid, color: {r: 10, g: 10, b: 10, a: 255}}
  - id: main_text
    text: {content: "VCR", size: 120, font_variant: line}
    position: {x: 640, y: 360}
    anchor: center
`, prompt, contextStr)

	requestBody, _ := json.Marshal(map[string]interface{}{
		"model": "local-model",
		"messages": []map[string]string{
			{"role": "user", "content": messaage},
		},
		"temperature": 0.2,
	})

	resp, err := http.Post("http://localhost:1234/v1/chat/completions", "application/json", bytes.NewBuffer(requestBody))
	if err != nil {
		emit(IPCMessage{Type: "status", Status: "Error connecting to LM Studio. Is it running on port 1234?"})
		os.Exit(1)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	var aiResp struct {
		Choices []struct {
			Message struct {
				Content string `json:"content"`
			} `json:"message"`
		} `json:"choices"`
	}
	json.Unmarshal(body, &aiResp)

	aiContent := aiResp.Choices[0].Message.Content
	// Extract YAML if it's in a code block
	re := regexp.MustCompile("(?s)```(?:yaml)?\n?(.*?)```")
	match := re.FindStringSubmatch(aiContent)
	yamlContent := aiContent
	if len(match) > 1 {
		yamlContent = match[1]
	}

	manifestPath := "agent_manifest.yaml"
	os.WriteFile(manifestPath, []byte(yamlContent), 0644)

	// 3. Render with VCR engine
	emit(IPCMessage{Type: "status", Status: "Generating Video with VCR Engine..."})

	// Run vcr build
	outputPath := "renders/agentic_output.mov"
	// Use the local debug binary
	vcrPath := "./target/debug/vcr"
	if _, err := os.Stat(vcrPath); err != nil {
		vcrPath = "vcr" // Fallback to path
	}

	cmd := exec.Command(vcrPath, "build", manifestPath, "-o", outputPath)
	stderr, _ := cmd.StderrPipe()
	cmd.Start()

	// Parse VCR engine progress
	scanner := bufio.NewScanner(stderr)
	for scanner.Scan() {
		line := scanner.Text()
		// Try to extract frame progress: "rendered frame 10/120"
		if strings.Contains(line, "rendered frame") {
			var current, total int
			fmt.Sscanf(line, "rendered frame %d/%d", &current, &total)
			if total > 0 {
				emit(IPCMessage{
					Type:    "progress",
					Percent: float64(current) / float64(total),
					Status:  fmt.Sprintf("Rendering %d/%d", current, total),
				})
			}
		}
	}
	cmd.Wait()

	emit(IPCMessage{
		Type:   "artifact",
		Path:   outputPath,
		Status: fmt.Sprintf("SUCCESS: Generated %s", outputPath),
	})
}
