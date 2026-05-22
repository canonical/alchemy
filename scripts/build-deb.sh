#!/usr/bin/env bash
# Build a .deb package for Alchemy
# Usage: build-deb.sh <target> <binary_path> <version> <output_dir> [filename_version]

set -euo pipefail

TARGET="${1:?target required (e.g., x86_64-unknown-linux-gnu)}"
BINARY_PATH="${2:?binary_path required}"
VERSION="${3:?version required}"
OUTPUT_DIR="${4:?output_dir required}"
FILENAME_VERSION="${5:-$VERSION}"

if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: binary not found at $BINARY_PATH" >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Map target to Debian architecture
case "$TARGET" in
    x86_64-unknown-linux-gnu)
        DEB_ARCH="amd64"
        ;;
    aarch64-unknown-linux-gnu)
        DEB_ARCH="arm64"
        ;;
    *)
        echo "Error: unsupported target for deb packaging: $TARGET" >&2
        exit 1
        ;;
esac

# Create debian package structure
DEB_ROOT="$WORK/alchemy-$VERSION"
mkdir -p "$DEB_ROOT/DEBIAN"
mkdir -p "$DEB_ROOT/usr/local/bin"

# Copy binary
cp "$BINARY_PATH" "$DEB_ROOT/usr/local/bin/alchemy"
chmod 0755 "$DEB_ROOT/usr/local/bin/alchemy"

# Create control file
cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: alchemy
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: Canonical <canonical@github.com>
Homepage: https://github.com/canonical/alchemy
Description: Cross-platform CI/CD AI agent
 Alchemy is a single static binary that works in two modes:
 - Pipe mode: for automation in CI/CD pipelines
 - TUI mode: for interactive terminal use
EOF

# Create postinst script (optional, for completion setup)
cat > "$DEB_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/bash
set -e

# Create symlink for compatibility if needed
if [ ! -e /usr/bin/alchemy ] && [ -e /usr/local/bin/alchemy ]; then
    ln -sf /usr/local/bin/alchemy /usr/bin/alchemy || true
fi

exit 0
EOF
chmod 0755 "$DEB_ROOT/DEBIAN/postinst"

# Build the deb package
if [ -n "$FILENAME_VERSION" ]; then
    PACKAGE_NAME="alchemy-${FILENAME_VERSION}_${DEB_ARCH}.deb"
else
    PACKAGE_NAME="alchemy_${DEB_ARCH}.deb"
fi
dpkg-deb --build "$DEB_ROOT" "$OUTPUT_DIR/$PACKAGE_NAME"

# Generate checksum
sha256sum "$OUTPUT_DIR/$PACKAGE_NAME" > "$OUTPUT_DIR/$PACKAGE_NAME.sha256"

echo "Created: $OUTPUT_DIR/$PACKAGE_NAME"
