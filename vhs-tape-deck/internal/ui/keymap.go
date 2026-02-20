package ui

import "github.com/charmbracelet/bubbles/key"

type keyMap struct {
	Up      key.Binding
	Down    key.Binding
	Insert  key.Binding
	Play    key.Binding
	Preview key.Binding
	Cancel  key.Binding
	DryRun  key.Binding
	Logs    key.Binding
	Help    key.Binding
	Quit    key.Binding
}

func newKeyMap() keyMap {
	return keyMap{
		Up:      key.NewBinding(key.WithKeys("up", "k"), key.WithHelp("↑/k", "previous tape")),
		Down:    key.NewBinding(key.WithKeys("down", "j"), key.WithHelp("↓/j", "next tape")),
		Insert:  key.NewBinding(key.WithKeys("enter"), key.WithHelp("enter", "insert/eject")),
		Play:    key.NewBinding(key.WithKeys(" "), key.WithHelp("space", "play")),
		Preview: key.NewBinding(key.WithKeys("p"), key.WithHelp("p", "preview frame")),
		Cancel:  key.NewBinding(key.WithKeys("ctrl+x"), key.WithHelp("ctrl+x", "cancel run")),
		DryRun:  key.NewBinding(key.WithKeys("d"), key.WithHelp("d", "toggle dry run")),
		Logs:    key.NewBinding(key.WithKeys("l"), key.WithHelp("l", "clear logs")),
		Help:    key.NewBinding(key.WithKeys("h", "?"), key.WithHelp("h/?", "toggle help")),
		Quit:    key.NewBinding(key.WithKeys("q", "ctrl+c"), key.WithHelp("q", "quit")),
	}
}

func (k keyMap) ShortHelp() []key.Binding {
	return []key.Binding{k.Insert, k.Play, k.Preview, k.Cancel, k.DryRun, k.Logs, k.Help, k.Quit}
}

func (k keyMap) FullHelp() [][]key.Binding {
	return [][]key.Binding{
		{k.Up, k.Down, k.Insert, k.Play, k.Cancel},
		{k.Preview, k.DryRun, k.Logs, k.Help, k.Quit},
	}
}
