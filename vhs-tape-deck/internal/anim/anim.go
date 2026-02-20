package anim

import (
	"fmt"
	"strings"
)

type State string

const (
	StateIdle     State = "idle"
	StateInserted State = "inserted"
	StateRunning  State = "running"
	StateSuccess  State = "success"
	StateFailed   State = "failed"
)

type Options struct {
	LabelStyle    string
	ShellColorway string
}

type CassetteAnimator struct{}

func NewCassetteAnimator() CassetteAnimator {
	return CassetteAnimator{}
}

func (a CassetteAnimator) Render(label, tapeID string, tickCount int, state State, isInserted bool, opts Options) string {
	if tickCount < 0 {
		tickCount = 0
	}

	shellChar := shellForColorway(opts.ShellColorway)
	reelLeft, reelRight := reelGlyphs(tickCount, state)
	labelText := styleLabel(label, opts.LabelStyle)
	idText := styleLabel(tapeID, "clean")
	status := statusBadge(state, isInserted)

	offset := 6
	if isInserted {
		offset = 0
		if tickCount < 6 {
			offset = 6 - tickCount
		}
	}
	indent := strings.Repeat(" ", offset)

	lines := []string{
		"+-------------------------------+",
		"|      VHS SLOT [====]          |",
		"+-------------------------------+",
		indent + "+---------------------------+",
		indent + "|" + strings.Repeat(shellChar, 27) + "|",
		indent + fmt.Sprintf("|  (%s)       [%s]      (%s) |", reelLeft, centerText(labelText, 7), reelRight),
		indent + fmt.Sprintf("|  ID:%s             |", centerText(idText, 20)),
		indent + "|" + strings.Repeat(shellChar, 27) + "|",
		indent + "+---------------------------+",
		indent + "   " + status,
	}

	if state != StateRunning {
		for i := range lines {
			if i >= 4 && i <= 7 {
				lines[i] = shimmer(lines[i], tickCount, i)
			}
		}
	}

	return strings.Join(lines, "\n")
}

func reelGlyphs(tick int, state State) (string, string) {
	if state == StateRunning {
		frames := []string{"|", "/", "-", "\\"}
		return frames[tick%len(frames)], frames[(tick+2)%len(frames)]
	}
	if state == StateSuccess {
		return "*", "*"
	}
	if state == StateFailed {
		return "x", "x"
	}
	return "o", "o"
}

func shellForColorway(colorway string) string {
	switch strings.ToLower(strings.TrimSpace(colorway)) {
	case "gray":
		return "="
	case "clear":
		return "."
	default:
		return "#"
	}
}

func styleLabel(label, style string) string {
	label = strings.TrimSpace(label)
	if label == "" {
		return "UNTITLED"
	}
	switch strings.ToLower(style) {
	case "noisy":
		return strings.ToUpper(label)
	case "handwritten":
		return strings.ToLower(label)
	default:
		return label
	}
}

func statusBadge(state State, inserted bool) string {
	switch state {
	case StateRunning:
		return "[ PLAY ]"
	case StateSuccess:
		return "[ DONE ]"
	case StateFailed:
		return "[ FAIL ]"
	case StateInserted:
		if inserted {
			return "[ READY ]"
		}
	}
	if inserted {
		return "[ LOADED ]"
	}
	return "[ EJECT ]"
}

func centerText(s string, width int) string {
	runes := []rune(s)
	if len(runes) > width {
		return string(runes[:width])
	}
	if len(runes) == width {
		return s
	}
	pad := width - len(runes)
	left := pad / 2
	right := pad - left
	return strings.Repeat(" ", left) + s + strings.Repeat(" ", right)
}

func shimmer(line string, tick, row int) string {
	idx := (tick + row*3) % len(line)
	if idx <= 0 || idx >= len(line)-1 {
		return line
	}
	b := []byte(line)
	if b[idx] == '#' || b[idx] == '=' || b[idx] == '.' {
		b[idx] = '~'
	}
	return string(b)
}
