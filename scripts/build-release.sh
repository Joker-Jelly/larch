#!/usr/bin/env bash
set -euo pipefail

# ── Extract version from Cargo.toml ─────────────────────────────────
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
DIST_DIR="dist"
BIN_NAME="larch"

echo "🌲 Building Larch v${VERSION}"
echo "================================"

# ── Targets ──────────────────────────────────────────────────────────
# Each entry: "rust-target:archive-suffix"
TARGETS=(
    "aarch64-apple-darwin:darwin-arm64"
    "x86_64-unknown-linux-gnu:linux-x86_64"
    "aarch64-unknown-linux-gnu:linux-arm64"
)

# ── Preflight checks ────────────────────────────────────────────────
check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        echo "❌ $1 not found. $2"
        exit 1
    fi
}

check_cmd cargo "Install Rust: https://rustup.rs"
check_cmd cargo-zigbuild "Run: cargo install cargo-zigbuild"
check_cmd zig "Run: brew install zig"

# Ensure all targets are installed
for entry in "${TARGETS[@]}"; do
    target="${entry%%:*}"
    if ! rustup target list --installed | grep -q "$target"; then
        echo "📦 Adding target: $target"
        rustup target add "$target"
    fi
done

# ── Build ────────────────────────────────────────────────────────────
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

HOST_TARGET=$(rustc -vV | grep '^host:' | awk '{print $2}')

for entry in "${TARGETS[@]}"; do
    target="${entry%%:*}"
    suffix="${entry##*:}"
    archive="${BIN_NAME}-v${VERSION}-${suffix}.tar.gz"

    echo ""
    echo "🔨 Building ${suffix} (${target})..."

    if [ "$target" = "$HOST_TARGET" ]; then
        # Native build — no zigbuild needed
        cargo build --release --target "$target"
    else
        cargo zigbuild --release --target "$target"
    fi

    tar czf "${DIST_DIR}/${archive}" -C "target/${target}/release" "$BIN_NAME"
    echo "   ✅ ${archive}"
done

# ── Summary ──────────────────────────────────────────────────────────
echo ""
echo "================================"
echo "📦 Packages in ${DIST_DIR}/:"
ls -lh "$DIST_DIR"/*.tar.gz
echo ""
echo "🏷️  Ready to tag and release: v${VERSION}"
echo "   git tag v${VERSION} && git push origin develop --tags"
