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
	"time"

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

// debug logs to a file so we can see what's happening without breaking IPC
func debugLog(msg string) {
	f, _ := os.OpenFile("vcr-agent.log", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	defer f.Close()
	f.WriteString(msg + "\n")
}

func main() {
	if len(os.Args) < 2 {
		emit(IPCMessage{Type: "status", Status: "Error: No prompt provided."})
		os.Exit(1)
	}
	prompt := os.Args[1]
	debugLog("--- STARTING AGENTIC RUN ---")
	debugLog("Prompt: " + prompt)

	// 1. Fetch Context from SQLite
	emit(IPCMessage{Type: "status", Status: "Reading Intelligence Tree..."})
	database, err := db.Open()
	if err != nil {
		emit(IPCMessage{Type: "status", Status: "Error opening DB"})
		os.Exit(1)
	}
	defer database.Conn.Close()

	nodes, _ := database.GetContextNodes()
	contextStr := strings.Join(nodes, "\n")

	// 2. Dynamic Model Detection from LM Studio
	emit(IPCMessage{Type: "status", Status: "Syncing with LM Studio..."})
	modelName := "local-model"
	if mResp, err := http.Get("http://127.0.0.1:1234/v1/models"); err == nil {
		var mData struct {
			Data []struct {
				ID string `json:"id"`
			} `json:"data"`
		}
		if body, err := io.ReadAll(mResp.Body); err == nil {
			json.Unmarshal(body, &mData)
			if len(mData.Data) > 0 {
				modelName = mData.Data[0].ID
				debugLog("Detected Model: " + modelName)
			}
		}
	}

	// 3. Query LLM
	emit(IPCMessage{Type: "status", Status: "Thinking... (Consulting local brain)"})

	systemPrompt := `You are the VCR Engine Brain. You only output valid VCR YAML manifests.
A VCR manifest MUST follow this structure:

version: 1
environment:
  resolution: {width: 1280, height: 720}
  fps: 24
  duration: 5.0
layers:
  - id: background
    procedural:
      kind: solid_color
      color: {r: 0.0, g: 0.0, b: 0.0, a: 1.0}
  - id: sample_text
    text:
      content: "HELLO"
      font_size: 120
      font_family: "GeistPixel-Line"
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
    position: {x: 640, y: 360}
    anchor: center

Rules:
1. No conversational text. 
2. Use "procedural" with "kind: solid_color" for backgrounds.
3. Colors (r, g, b, a) are 0.0 to 1.0. 
4. Use ONLY font_family: "GeistPixel-Line".
5. Resolution and position are integers.`

	userMessage := fmt.Sprintf(`Creative Context from Intelligence Tree:
%s

User Request: %s

Generate the YAML manifest now:`, contextStr, prompt)

	requestBody, _ := json.Marshal(map[string]interface{}{
		"model": modelName,
		"messages": []map[string]string{
			{"role": "system", "content": systemPrompt},
			{"role": "user", "content": userMessage},
		},
		"temperature": 0.0, // Strictness
	})

	client := &http.Client{Timeout: 60 * time.Second} // Allow 60s for LLM thought
	resp, err := client.Post("http://127.0.0.1:1234/v1/chat/completions", "application/json", bytes.NewBuffer(requestBody))
	if err != nil {
		emit(IPCMessage{Type: "status", Status: "LM Studio Request Timed Out or Failed."})
		os.Exit(1)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	debugLog("Raw AI Output: " + string(body))

	var aiResp struct {
		Choices []struct {
			Message struct {
				Content string `json:"content"`
			} `json:"message"`
		} `json:"choices"`
	}
	json.Unmarshal(body, &aiResp)

	if len(aiResp.Choices) == 0 {
		emit(IPCMessage{Type: "status", Status: "Deeply sorry: The model returned nothing."})
		os.Exit(1)
	}

	content := aiResp.Choices[0].Message.Content

	// Robust Extraction Logic: Prioritize finding the VCR version marker
	yamlContent := content
	if strings.Contains(content, "version:") {
		// Find where version: starts
		idx := strings.Index(content, "version:")
		yamlContent = content[idx:]
		// If there's a trailing code block marker, strip it
		if strings.Contains(yamlContent, "```") {
			yamlContent = strings.Split(yamlContent, "```")[0]
		}
	} else if strings.Contains(content, "```") {
		// Fallback to code block extraction
		re := regexp.MustCompile("(?s)```(?:yaml)?\n?(.*?)```")
		match := re.FindStringSubmatch(content)
		if len(match) > 1 {
			yamlContent = match[1]
		}
	}
	yamlContent = strings.TrimSpace(yamlContent)

	manifestPath := "agent_manifest.yaml"
	os.WriteFile(manifestPath, []byte(yamlContent), 0644)
	debugLog("Final Manifest:\n" + yamlContent)

	// 4. Render
	emit(IPCMessage{Type: "status", Status: "VCR Engine: Initializing GPU render..."})

	vcrPath := "./target/debug/vcr"
	outputPath := "renders/agentic_result.mov"
	os.MkdirAll("renders", 0755)

	cmd := exec.Command(vcrPath, "build", manifestPath, "-o", outputPath)
	stderr, _ := cmd.StderrPipe()
	cmd.Start()

	scanner := bufio.NewScanner(stderr)
	for scanner.Scan() {
		line := scanner.Text()
		debugLog("VCR Build Log: " + line)
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
		Status: "RENDER COMPLETE: Saved to " + outputPath,
	})
}
