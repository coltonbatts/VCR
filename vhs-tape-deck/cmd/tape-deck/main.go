package main

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"vhs-tape-deck/internal/config"
	"vhs-tape-deck/internal/ui"
)

func main() {
	os.Exit(run(os.Args[1:]))
}

func run(args []string) int {
	if len(args) == 0 {
		return runUI("")
	}

	switch args[0] {
	case "init":
		return initConfig(args[1:])
	case "run":
		configPath := ""
		fs := flag.NewFlagSet("run", flag.ContinueOnError)
		fs.StringVar(&configPath, "config", "", "path to config yaml")
		if err := fs.Parse(args[1:]); err != nil {
			fmt.Fprintln(os.Stderr, err)
			return 2
		}
		return runUI(configPath)
	case "help", "-h", "--help":
		printUsage()
		return 0
	default:
		printUsage()
		return 2
	}
}

func initConfig(args []string) int {
	var configPath string
	var force bool

	fs := flag.NewFlagSet("init", flag.ContinueOnError)
	fs.StringVar(&configPath, "config", "", "path to config yaml")
	fs.BoolVar(&force, "force", false, "overwrite existing config")
	if err := fs.Parse(args); err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 2
	}

	if configPath == "" {
		var err error
		configPath, err = config.DefaultConfigPath()
		if err != nil {
			fmt.Fprintf(os.Stderr, "resolve config path: %v\n", err)
			return 1
		}
	}

	cwd, err := os.Getwd()
	if err != nil {
		fmt.Fprintf(os.Stderr, "resolve cwd: %v\n", err)
		return 1
	}

	if err := config.WriteStarterConfig(configPath, cwd, force); err != nil {
		fmt.Fprintf(os.Stderr, "init config: %v\n", err)
		return 1
	}

	abs, _ := filepath.Abs(configPath)
	fmt.Printf("wrote starter config: %s\n", abs)
	return 0
}

func runUI(configPath string) int {
	if configPath == "" {
		var err error
		configPath, err = config.DefaultConfigPath()
		if err != nil {
			fmt.Fprintf(os.Stderr, "resolve config path: %v\n", err)
			return 1
		}
	}

	cwd, err := os.Getwd()
	if err != nil {
		fmt.Fprintf(os.Stderr, "resolve cwd: %v\n", err)
		return 1
	}

	cfg, err := config.Load(configPath, cwd)
	if err != nil {
		fmt.Fprintf(os.Stderr, "load config (%s): %v\n", configPath, err)
		fmt.Fprintln(os.Stderr, "tip: run `tape-deck init` to create a starter config")
		return 1
	}

	if err := ui.Run(cfg); err != nil {
		fmt.Fprintf(os.Stderr, "run UI: %v\n", err)
		return 1
	}
	return 0
}

func printUsage() {
	fmt.Println(`tape-deck - VHS Tape Deck UI for VCR

Usage:
  tape-deck init [--config <path>] [--force]
  tape-deck run [--config <path>]
  tape-deck

Commands:
  init    Write a starter config with five tapes
  run     Start the Tape Deck UI

If no command is provided, run is implied.`)
}
