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
PREVIOUS_DIR="$INSTALL_DIR.previous"
REPO_URL="${VCR_REPO_URL:-https://github.com/coltonbatts/VCR.git}"
BACKUP_DIR=""
INSTALL_OK=0

# The binary records its build directory (it locates bundled fonts there), so the new copy must be
# built in its final location. To keep a working install safe, the old one is moved aside first and
# restored on ANY failure (clone, build, interruption).
restore_previous() {
  if [ "$INSTALL_OK" -ne 1 ] && [ -n "$BACKUP_DIR" ] && [ -d "$BACKUP_DIR" ]; then
    echo -e "${YELLOW}Install did not complete; restoring your previous installation.${RESET}"
    rm -rf "$INSTALL_DIR"
    mv "$BACKUP_DIR" "$INSTALL_DIR"
    echo -e "Restored ${BOLD}$INSTALL_DIR${RESET}"
  fi
}
trap restore_previous EXIT

mkdir -p "$HOME/.vcr"
if [ -e "$INSTALL_DIR" ]; then
  BACKUP_DIR="$INSTALL_DIR.backup.$$"
  echo -e "Moving existing installation aside to ${BOLD}$BACKUP_DIR${RESET} (restored automatically if anything fails)..."
  mv "$INSTALL_DIR" "$BACKUP_DIR"
fi

echo -e "Cloning ${BOLD}$REPO_URL${RESET}..."
git clone "$REPO_URL" "$INSTALL_DIR"

cd "$INSTALL_DIR"

# --- Build ---

echo -e "Building ${BOLD}vcr${RESET} in release mode (this may take a few minutes)..."
cargo build --release

if [ ! -x "$INSTALL_DIR/target/release/vcr" ]; then
  echo -e "${RED}Error: build finished but target/release/vcr was not produced.${RESET}"
  exit 1
fi

# New install is good: keep exactly one previous generation (never silently delete user data such
# as renders/ inside the old install), and drop any older leftovers.
if [ -n "$BACKUP_DIR" ]; then
  rm -rf "$PREVIOUS_DIR"
  mv "$BACKUP_DIR" "$PREVIOUS_DIR"
  echo -e "Previous installation kept at ${BOLD}$PREVIOUS_DIR${RESET} (delete it when you no longer need it)."
fi
INSTALL_OK=1
trap - EXIT

# --- Symlink ---

# Replace an existing symlink, but never overwrite a regular file someone else put there.
link_binary() {
  local target="$1" link="$2"
  if [ -e "$link" ] && [ ! -L "$link" ]; then
    echo -e "${YELLOW}$link exists and is not a symlink; leaving it untouched.${RESET}"
    echo "Run VCR directly from $target or remove that file and re-run this installer."
    return 0
  fi
  ln -sfn "$target" "$link"
}

BINARY_PATH="$INSTALL_DIR/target/release/vcr"
PRIMARY_BIN_DIR="/usr/local/bin"
FALLBACK_BIN_DIR="$HOME/.local/bin"
SHELL_NAME="$(basename "${SHELL:-}")"

echo -e "${GREEN}Build successful!${RESET}"

if [ -d "$PRIMARY_BIN_DIR" ] && [ -w "$PRIMARY_BIN_DIR" ]; then
  LINK_PATH="$PRIMARY_BIN_DIR/vcr"
  echo -e "Creating symlink at ${BOLD}$LINK_PATH${RESET}..."
  link_binary "$BINARY_PATH" "$LINK_PATH"
  echo -e "${BOLD}${GREEN}VCR is now installed!${RESET}"
else
  echo -e "${YELLOW}Warning: $PRIMARY_BIN_DIR is not writable.${RESET}"
  echo -e "Using fallback install location: ${BOLD}$FALLBACK_BIN_DIR${RESET}"
  mkdir -p "$FALLBACK_BIN_DIR"
  LINK_PATH="$FALLBACK_BIN_DIR/vcr"
  link_binary "$BINARY_PATH" "$LINK_PATH"

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

echo "Try running: vcr --version   (then: vcr capabilities --json)"
echo -e "\n${BOLD}${CYAN}Happy Rendering!${RESET}"
