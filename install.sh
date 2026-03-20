#!/bin/sh

set -e

BIN_NAME="larch"
BASE_URL="https://code.byted.org/tiktok/larch/raw/master"

# Determine install directory
if [ -d "$HOME/.local/bin" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    SUDO=""
elif [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
    SUDO=""
elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
    INSTALL_DIR="/usr/local/bin"
    SUDO="sudo"
else
    INSTALL_DIR="$HOME/.local/bin"
    SUDO=""
fi

# Detect OS and Architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

# Normalize arch: keep x86_64 as-is, map aarch64 → arm64
if [ "$ARCH" = "aarch64" ]; then
    ARCH="arm64"
elif [ "$ARCH" != "x86_64" ] && [ "$ARCH" != "arm64" ]; then
    echo "Unsupported architecture: $ARCH"
    exit 1
fi

if [ "$OS" != "linux" ] && [ "$OS" != "darwin" ]; then
    echo "Unsupported operating system: $OS"
    exit 1
fi

# Fetch version from Cargo.toml
echo "Fetching latest version..."
VERSION=$(curl -fsSL "$BASE_URL/Cargo.toml" | grep '^version' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$VERSION" ]; then
    echo "Failed to determine version from Cargo.toml."
    exit 1
fi

echo "Found version: $VERSION"

ASSET_NAME="${BIN_NAME}-v${VERSION}-${OS}-${ARCH}.tar.gz"
DOWNLOAD_URL="${BASE_URL}/dist/${ASSET_NAME}"

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
echo "✅ Larch v${VERSION} has been successfully installed to $INSTALL_DIR/$BIN_NAME"

# Check if INSTALL_DIR is in PATH
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    echo "⚠️  WARNING: $INSTALL_DIR is not in your PATH."
    echo "Add the following line to your ~/.bashrc or ~/.zshrc:"
    echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
fi

# Ask whether to install as a system service
echo ""
printf "Would you like to install larch as a system service? [y/N] "
read -r REPLY
case "$REPLY" in
    [yY]|[yY][eE][sS])
        echo "Running 'larch service install'..."
        "$INSTALL_DIR/$BIN_NAME" service install
        ;;
    *)
        echo "Skipping service installation. You can run 'larch service install' later."
        ;;
esac
