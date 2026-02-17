#!/bin/bash

# VCR — Video Component Renderer
# Install Script

set -e

RESET="\033[0m"
BOLD="\033[1m"
GREEN="\033[32m"
YELLOW="\033[33m"
RED="\033[31m"
CYAN="\033[36m"

echo -e "${BOLD}${CYAN}"
echo "██╗   ██╗ ██████╗██████╗ "
echo "██║   ██║██╔════╝██╔══██╗"
echo "██║   ██║██║     ██████╔╝"
echo "╚██╗ ██╔╝██║     ██╔══██╗"
echo "  ╚████╔╝ ╚██████╗██║  ██║"
echo "   ╚═══╝   ╚═════╝╚═╝  ╚═╝"
echo -e "${RESET}"

echo -e "${BOLD}Welcome to the VCR installation script.${RESET}"
echo "------------------------------------------------"

# --- Dependency Checks ---

# Git
if ! [ -x "$(command -v git)" ]; then
  echo -e "${RED}Error: git is not installed.${RESET}"
  exit 1
fi

# Cargo / Rust
if ! [ -x "$(command -v cargo)" ]; then
  echo -e "${YELLOW}Warning: Rust/Cargo is not installed.${RESET}"
  echo "VCR requires a stable Rust toolchain to build from source."
  echo -e "Please install it via ${BOLD}rustup${RESET}: https://rustup.rs/"
  echo "Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

# FFmpeg
if ! [ -x "$(command -v ffmpeg)" ]; then
  echo -e "${YELLOW}Warning: FFmpeg was not found on your PATH.${RESET}"
  echo "VCR requires FFmpeg for video encoding (ProRes export)."
  echo "You can still install VCR, but rendering video will fail until FFmpeg is installed."
fi

# --- Install Path ---

INSTALL_DIR="$HOME/.vcr/VCR"
echo -e "Cleaning up previous installations at ${BOLD}$INSTALL_DIR${RESET}..."
rm -rf "$INSTALL_DIR"
mkdir -p "$HOME/.vcr"

echo -e "Cloning ${BOLD}coltonbatts/VCR${RESET}..."
git clone https://github.com/coltonbatts/VCR.git "$INSTALL_DIR"

cd "$INSTALL_DIR"

# --- Build ---

echo -e "Building ${BOLD}vcr${RESET} in release mode (this may take a few minutes)..."
cargo build --release

# --- Symlink ---

BINARY_PATH="$INSTALL_DIR/target/release/vcr"
PRIMARY_BIN_DIR="/usr/local/bin"
FALLBACK_BIN_DIR="$HOME/.local/bin"
SHELL_NAME="$(basename "${SHELL:-}")"

echo -e "${GREEN}Build successful!${RESET}"

if [ -d "$PRIMARY_BIN_DIR" ] && [ -w "$PRIMARY_BIN_DIR" ]; then
  LINK_PATH="$PRIMARY_BIN_DIR/vcr"
  echo -e "Creating symlink at ${BOLD}$LINK_PATH${RESET}..."
  rm -f "$LINK_PATH"
  ln -s "$BINARY_PATH" "$LINK_PATH"
  echo -e "${BOLD}${GREEN}VCR is now installed!${RESET}"
else
  echo -e "${YELLOW}Warning: $PRIMARY_BIN_DIR is not writable.${RESET}"
  echo -e "Using fallback install location: ${BOLD}$FALLBACK_BIN_DIR${RESET}"
  mkdir -p "$FALLBACK_BIN_DIR"
  LINK_PATH="$FALLBACK_BIN_DIR/vcr"
  rm -f "$LINK_PATH"
  ln -s "$BINARY_PATH" "$LINK_PATH"

  case "$SHELL_NAME" in
    zsh)
      SHELL_RC="$HOME/.zshrc"
      ;;
    bash)
      SHELL_RC="$HOME/.bashrc"
      ;;
    *)
      SHELL_RC="$HOME/.profile"
      ;;
  esac

  if [[ ":$PATH:" != *":$FALLBACK_BIN_DIR:"* ]]; then
    echo -e "${YELLOW}$FALLBACK_BIN_DIR is not on your PATH for this shell.${RESET}"
    echo "Add it with:"
    echo -e "${BOLD}echo 'export PATH=\"$FALLBACK_BIN_DIR:\$PATH\"' >> \"$SHELL_RC\"${RESET}"
    echo -e "${BOLD}source \"$SHELL_RC\"${RESET}"
  fi
fi

echo "Try running: vcr --version"
echo -e "\n${BOLD}${CYAN}Happy Rendering!${RESET}"
