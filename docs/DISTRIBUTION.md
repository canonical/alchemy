# Alchemy Distribution & Package Management

This document describes the various ways to install and distribute Alchemy across different platforms and package managers.

## Overview

Alchemy is available in the following formats:

| Format | Platform | Use Case |
|--------|----------|----------|
| `.pkg` | macOS (universal) | Standard macOS installation |
| `.exe` | Windows | Standard Windows installation |
| `.deb` | Debian/Ubuntu/Linux Mint | APT package manager |
| `.rpm` | Fedora/RHEL/CentOS/Rocky | DNF/YUM package manager |
| `tar.gz` | Linux/macOS (raw binary) | Manual installation |
| Docker | All platforms | Container-based execution |
| Homebrew tap | macOS | Brew package manager (planned) |
| Launchpad PPA | Ubuntu | Ubuntu software repositories (planned) |
| Fedora Copr | Fedora | Community-built repositories (planned) |
| AUR | Arch Linux | User-maintained repository (community) |

## Installation Methods

### Quick Install (All Platforms)

**macOS / Linux:**
```bash
curl -fsSL https://raw.githubusercontent.com/canonical/alchemy/refs/heads/main/install.sh | bash
```

**Windows (PowerShell):**
```powershell
powershell -c "irm https://raw.githubusercontent.com/canonical/alchemy/refs/heads/main/install.ps1 | iex"
```

### Package Managers

#### macOS (Homebrew)
```bash
# Via Homebrew tap (when available)
brew install canonical/alchemy/alchemy

# Direct .pkg installation
# Download from: https://github.com/canonical/alchemy/releases
```

#### Ubuntu / Debian
```bash
# Via APT (when PPA is available)
sudo apt update
sudo apt install alchemy

# Direct .deb installation
wget https://github.com/canonical/alchemy/releases/download/latest/alchemy-latest_amd64.deb
sudo dpkg -i alchemy-latest_amd64.deb
```

**Supported architectures:**
- `amd64` (Intel/AMD x86_64)
- `arm64` (ARM64/Apple Silicon)

#### Fedora / RHEL / CentOS / Rocky
```bash
# Via DNF (when Copr repo is available)
sudo dnf install alchemy

# Via YUM (older RHEL systems)
sudo yum install alchemy

# Direct .rpm installation
sudo rpm -ivh https://github.com/canonical/alchemy/releases/download/latest/alchemy-latest.x86_64.rpm
```

**Supported architectures:**
- `x86_64` (Intel/AMD)
- `aarch64` (ARM64)

#### Arch Linux
```bash
# Via AUR (community-maintained, when available)
yay -S alchemy
```

### Docker / Container

Run directly from the published OCI image:

```bash
# OpenRouter
docker run \
  --env ALCHEMY_PROVIDER=openrouter \
  --env ALCHEMY_API_KEY=sk-or-v1-... \
  --env ALCHEMY_MODEL=google/gemma-4-31b-it \
  --rm -it ghcr.io/canonical/alchemy:latest tui

# GitHub Copilot
docker run \
  --env ALCHEMY_PROVIDER=github-copilot \
  --env ALCHEMY_API_KEY=ghu_... \
  --env ALCHEMY_MODEL=claude-sonnet-4.6 \
  --rm -it ghcr.io/canonical/alchemy:latest tui

# Gemini
docker run \
  --env ALCHEMY_PROVIDER=gemini \
  --env ALCHEMY_API_KEY=AI... \
  --env ALCHEMY_MODEL=gemini-3.1-flash-lite-preview \
  --rm -it ghcr.io/canonical/alchemy:latest tui
```

## Release Process

### Automated Release Workflow

When you push a tag or to `main`, the GitHub Actions workflow automatically:

1. **Builds** binaries for all platforms:
   - x86_64-unknown-linux-gnu
   - aarch64-unknown-linux-gnu
   - x86_64-pc-windows-msvc
   - x86_64-apple-darwin
   - aarch64-apple-darwin

2. **Packages** into native formats:
   - macOS: Universal `.pkg` (arm64 + x86_64)
   - Windows: `.exe` installer (x86_64)
   - Linux: `.deb` packages (x86_64, arm64)
   - Linux: `.rpm` packages (x86_64, aarch64)

3. **Generates** checksums (SHA-256) for all artifacts

4. **Publishes** to GitHub Releases

### Build Scripts

#### DEB Packaging

**Location:** `scripts/build-deb.sh`

**Usage:**
```bash
./scripts/build-deb.sh <target> <binary_path> <version> <output_dir> [filename_version]
```

**Example:**
```bash
./scripts/build-deb.sh \
  "x86_64-unknown-linux-gnu" \
  "target/release/alchemy" \
  "0.1.0" \
  "dist/deb" \
  "latest"
```

**Requirements:**
- `dpkg` and `dpkg-dev` (Ubuntu: `sudo apt-get install dpkg-dev`)

**Output:**
- `alchemy-<filename_version>_<arch>.deb` (defaults to `<version>`)
- `alchemy-<filename_version>_<arch>.deb.sha256`

#### RPM Packaging

**Location:** `scripts/build-rpm.sh`

**Usage:**
```bash
./scripts/build-rpm.sh <target> <binary_path> <version> <output_dir> [filename_version]
```

**Example:**
```bash
./scripts/build-rpm.sh \
  "x86_64-unknown-linux-gnu" \
  "target/release/alchemy" \
  "0.1.0" \
  "dist/rpm" \
  "latest"
```

**Requirements:**
- `rpm-build` and `rpmbuild` (Fedora: `sudo dnf install rpm-build`)

**Output:**
- `alchemy-<filename_version>.<arch>.rpm` (defaults to `<version>`)
- `alchemy-<filename_version>.<arch>.rpm.sha256`

## Verification

### Verify Package Integrity

**DEB:**
```bash
# Check package contents
dpkg-deb -c alchemy-latest_amd64.deb

# Check metadata
dpkg-deb -I alchemy-latest_amd64.deb

# Verify checksum
sha256sum -c alchemy-latest_amd64.deb.sha256
```

**RPM:**
```bash
# Check package contents
rpm -qpl alchemy-latest.x86_64.rpm

# Check package info
rpm -qi alchemy-latest.x86_64.rpm

# Verify checksum
sha256sum -c alchemy-latest.x86_64.rpm.sha256
```

### Test Installation

**DEB:**
```bash
sudo dpkg -i alchemy-latest_amd64.deb
which alchemy
alchemy --help
```

**RPM:**
```bash
sudo rpm -ivh alchemy-latest.x86_64.rpm
which alchemy
alchemy --help
```

## Future Enhancements

### Planned Package Managers

- **Homebrew Tap:** Automated tap updates on release
- **Ubuntu PPA (Launchpad):** APT repository with auto-updates
- **Fedora Copr:** Community repo with auto-builds
- **AUR (Arch User Repository):** Community-maintained package

### Future Distribution Methods

- Package repository signing (GPG signatures)
- Software Bill of Materials (SBOM)
- Container image signing & attestation
- Nix package manager support
- GuixSD package manager support

## Troubleshooting

### DEB Installation Issues

**Package conflicts:**
```bash
# Check for conflicts
apt-cache policy alchemy

# Force installation (use with caution)
sudo dpkg -i --force-overwrite alchemy-latest_amd64.deb
```

**Uninstall:**
```bash
sudo apt remove alchemy
```

### RPM Installation Issues

**Package conflicts:**
```bash
# Check for conflicts
rpm -qi alchemy

# Force installation (use with caution)
sudo rpm -i --force alchemy-latest.x86_64.rpm
```

**Uninstall:**
```bash
sudo rpm -e alchemy
```

### Binary Not in PATH

The packages install to `/usr/local/bin/alchemy` with a symlink at `/usr/bin/alchemy`.

**If binary is not found:**
```bash
# Check installation
ls -la /usr/local/bin/alchemy
ls -la /usr/bin/alchemy

# Add to PATH if needed
export PATH="/usr/local/bin:$PATH"

# Or reinstall
sudo dpkg -i --reinstall alchemy-latest_amd64.deb
```

## Release Checklist

When publishing a release:

1. ✅ Update version in `Cargo.toml`
2. ✅ Update CHANGELOG (if maintained)
3. ✅ Create a git tag: `git tag -a v0.1.0 -m "Release 0.1.0"`
4. ✅ Push tag: `git push origin v0.1.0`
5. ✅ Wait for GitHub Actions workflow to complete
6. ✅ Verify all artifacts are published to GitHub Releases
7. ✅ Verify checksums are present and valid
8. ✅ Test installation from each package format (optional)

## References

- [GitHub Releases API](https://docs.github.com/en/rest/releases)
- [Debian Package Format](https://wiki.debian.org/DebianPackageFormat)
- [RPM Package Format](https://rpm.org/)
- [Homebrew Tap Documentation](https://docs.brew.sh/Taps)
- [Ubuntu Launchpad](https://launchpad.net/)
- [Fedora Copr](https://copr.fedorainfracloud.org/)
- [AUR Guidelines](https://wiki.archlinux.org/title/AUR)
