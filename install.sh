#!/bin/sh

set -e

# Default settings
REPO="Joker-Jelly/larch"
BIN_NAME="larch"

# Determine install directory
if [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
    SUDO=""
elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
    INSTALL_DIR="/usr/local/bin"
    SUDO="sudo"
else
    # Fallback to user local bin if no sudo access or prefer user space
    INSTALL_DIR="$HOME/.local/bin"
    SUDO=""
fi

# Override to ~/.local/bin per best practices if it already exists or if we prefer user-space
if [ -d "$HOME/.local/bin" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    SUDO=""
fi

# Detect OS and Architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

if [ "$ARCH" = "x86_64" ]; then
    ARCH="amd64"
elif [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
    ARCH="arm64"
else
    echo "Unsupported architecture: $ARCH"
    exit 1
fi

if [ "$OS" != "linux" ] && [ "$OS" != "darwin" ]; then
    echo "Unsupported operating system: $OS"
    exit 1
fi

echo "Detecting latest release for $OS $ARCH..."

# Get latest release tag from GitHub API
LATEST_TAG=$(curl -s https://api.github.com/repos/$REPO/releases/latest | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST_TAG" ]; then
    echo "Failed to fetch latest release from GitHub."
    exit 1
fi

echo "Found latest version: $LATEST_TAG"

# Assuming standard naming convention for your release assets.
ASSET_NAME="${BIN_NAME}-${OS}-${ARCH}.tar.gz"
# Windows assets are zip files in the workflow
if [ "$OS" = "windows" ]; then
    ASSET_NAME="${BIN_NAME}-${OS}-${ARCH}.zip"
fi
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${ASSET_NAME}"

# Temp dir for extraction
TMP_DIR=$(mktemp -d)
cd "$TMP_DIR"

echo "Downloading $DOWNLOAD_URL..."
curl -fsSL "$DOWNLOAD_URL" -o "$ASSET_NAME"

echo "Extracting..."
tar -xzf "$ASSET_NAME"

# Ensure target directory exists
if [ ! -d "$INSTALL_DIR" ]; then
    echo "Creating directory $INSTALL_DIR..."
    $SUDO mkdir -p "$INSTALL_DIR"
fi

echo "Installing $BIN_NAME to $INSTALL_DIR..."
$SUDO mv "$BIN_NAME" "$INSTALL_DIR/"

# Cleanup
cd - > /dev/null
rm -rf "$TMP_DIR"

echo ""
echo "✅ Larch has been successfully installed to $INSTALL_DIR/$BIN_NAME"

# Check if INSTALL_DIR is in PATH
if echo "$PATH" | grep -q "$INSTALL_DIR"; then
    echo "Run 'larch --help' to get started."
else
    echo "⚠️  WARNING: $INSTALL_DIR is not in your PATH."
    echo "Add the following line to your ~/.bashrc or ~/.zshrc:"
    echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
fi
