package anim

import (
	"strings"
	"testing"
)

func TestRenderDeterministic(t *testing.T) {
	t.Parallel()

	a := NewCassetteAnimator()
	left := a.Render("Alpha", "alpha", 3, StateRunning, true, Options{LabelStyle: "clean", ShellColorway: "black"})
	right := a.Render("Alpha", "alpha", 3, StateRunning, true, Options{LabelStyle: "clean", ShellColorway: "black"})
	if left != right {
		t.Fatalf("expected deterministic output")
	}
}

func TestRenderSampleFrames(t *testing.T) {
	t.Parallel()

	a := NewCassetteAnimator()

	frame0 := a.Render("Alpha", "alpha", 0, StateRunning, true, Options{LabelStyle: "clean", ShellColorway: "gray"})
	if !strings.Contains(frame0, "[ PLAY ]") {
		t.Fatalf("expected running status badge in frame0, got:\n%s", frame0)
	}
	if !strings.Contains(frame0, "(|)") {
		t.Fatalf("expected left reel glyph for tick0, got:\n%s", frame0)
	}
	if !strings.Contains(frame0, "Alpha") {
		t.Fatalf("expected label text in frame0, got:\n%s", frame0)
	}

	frame1 := a.Render("Alpha", "alpha", 1, StateRunning, true, Options{LabelStyle: "clean", ShellColorway: "gray"})
	if !strings.Contains(frame1, "(/)") {
		t.Fatalf("expected left reel glyph for tick1, got:\n%s", frame1)
	}
}

func TestRenderIdleEjected(t *testing.T) {
	t.Parallel()

	a := NewCassetteAnimator()
	frame := a.Render("Calm", "calm-id", 9, StateIdle, false, Options{LabelStyle: "handwritten", ShellColorway: "clear"})
	if !strings.Contains(frame, "[ EJECT ]") {
		t.Fatalf("expected eject status, got:\n%s", frame)
	}
	if !strings.Contains(frame, "      +---------------------------+") {
		t.Fatalf("expected tape body with ejected offset, got:\n%s", frame)
	}
}
