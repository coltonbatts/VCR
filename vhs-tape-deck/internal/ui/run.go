package ui

import (
	tea "github.com/charmbracelet/bubbletea"

	"vhs-tape-deck/internal/config"
	"vhs-tape-deck/internal/runner"
)

func Run(cfg *config.Config) error {
	m := NewModel(cfg, runner.New(nil))
	p := tea.NewProgram(m, tea.WithAltScreen())
	_, err := p.Run()
	return err
}
