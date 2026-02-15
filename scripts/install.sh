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
LOCAL_BIN="/usr/local/bin/vcr"

echo -e "${GREEN}Build successful!${RESET}"

if [ -w "/usr/local/bin" ]; then
  echo -e "Creating symlink at ${BOLD}$LOCAL_BIN${RESET}..."
  rm -f "$LOCAL_BIN"
  ln -s "$BINARY_PATH" "$LOCAL_BIN"
  echo -e "${BOLD}${GREEN}VCR is now installed!${RESET}"
  echo "Try running: vcr --version"
else
  echo -e "${YELLOW}Warning: /usr/local/bin is not writable.${RESET}"
  echo "You can manually add the VCR binary to your PATH:"
  echo -e "${BOLD}export PATH=\"\$PATH:$INSTALL_DIR/target/release\"${RESET}"
fi

echo -e "\n${BOLD}${CYAN}Happy Rendering!${RESET}"
