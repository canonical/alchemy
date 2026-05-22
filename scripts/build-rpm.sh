#!/usr/bin/env bash
# Build a .rpm package for Alchemy
# Usage: build-rpm.sh <target> <binary_path> <version> <output_dir> [filename_version]

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

# Map target to RPM architecture
case "$TARGET" in
    x86_64-unknown-linux-gnu)
        RPM_ARCH="x86_64"
        ;;
    aarch64-unknown-linux-gnu)
        RPM_ARCH="aarch64"
        ;;
    *)
        echo "Error: unsupported target for rpm packaging: $TARGET" >&2
        exit 1
        ;;
esac

# Create RPM build directory structure
BUILD_ROOT="$WORK/buildroot"
mkdir -p "$BUILD_ROOT/usr/local/bin"
mkdir -p "$BUILD_ROOT/usr/bin"  # For easier PATH discovery

# Copy binary
cp "$BINARY_PATH" "$BUILD_ROOT/usr/local/bin/alchemy"
chmod 0755 "$BUILD_ROOT/usr/local/bin/alchemy"

# Create symlink in /usr/bin for convenience
ln -sf /usr/local/bin/alchemy "$BUILD_ROOT/usr/bin/alchemy"

# Create RPM spec file
cat > "$WORK/alchemy.spec" <<'EOF'
Name:           alchemy
Version:        %{version}
Release:        1%{?dist}
Summary:        Cross-platform CI/CD AI agent
License:        MIT
URL:            https://github.com/canonical/alchemy

%description
Alchemy is a single static binary that works in two modes:
- Pipe mode: for automation in CI/CD pipelines
- TUI mode: for interactive terminal use

%files
/usr/local/bin/alchemy
/usr/bin/alchemy

%post
# Ensure binary is executable
chmod 0755 /usr/local/bin/alchemy
exit 0

%changelog
* Thu May 22 2024 Canonical <canonical@github.com> - %{version}-1
- Initial release
EOF

# Build the RPM package
rpmbuild -bb \
    --define "_topdir $WORK/rpmbuild" \
    --define "buildroot $BUILD_ROOT" \
    --define "version $VERSION" \
    --define "_binaries_in_noarch_packages_terminate_build 0" \
    "$WORK/alchemy.spec"

# Find the built RPM
BUILT_RPM=$(find "$WORK/rpmbuild/RPMS" -name "*.rpm" | head -n 1)
if [ -z "$BUILT_RPM" ]; then
    echo "Error: RPM build failed, no output found" >&2
    exit 1
fi

# Copy to output directory with standardized name
if [ -n "$FILENAME_VERSION" ]; then
    PACKAGE_NAME="alchemy-${FILENAME_VERSION}.${RPM_ARCH}.rpm"
else
    PACKAGE_NAME="alchemy-${RPM_ARCH}.rpm"
fi
cp "$BUILT_RPM" "$OUTPUT_DIR/$PACKAGE_NAME"

# Generate checksum
sha256sum "$OUTPUT_DIR/$PACKAGE_NAME" > "$OUTPUT_DIR/$PACKAGE_NAME.sha256"

echo "Created: $OUTPUT_DIR/$PACKAGE_NAME"
