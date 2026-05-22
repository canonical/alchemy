#!/usr/bin/env bash
set -euo pipefail

REPO="${ALCHEMY_INSTALL_REPO:-canonical/alchemy}"
VERSION="${ALCHEMY_VERSION:-latest}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

require_cmd curl
require_cmd tar
require_cmd uname

os="$(uname -s)"
arch="$(uname -m)"

case "${os}" in
  Linux) os_target="unknown-linux-gnu" ;;
  Darwin) os_target="apple-darwin" ;;
  *)
    echo "Unsupported OS: ${os}. This installer supports Linux and macOS." >&2
    exit 1
    ;;
esac

case "${arch}" in
  x86_64|amd64) arch_target="x86_64" ;;
  arm64|aarch64) arch_target="aarch64" ;;
  *)
    echo "Unsupported architecture: ${arch}. Supported: x86_64, arm64/aarch64." >&2
    exit 1
    ;;
esac

if [ "${VERSION}" = "latest" ]; then
  tag_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest")"
  case "${tag_url}" in
    */releases/tag/*) ;;
    *)
      echo "Failed to resolve latest release tag for ${REPO}." >&2
      exit 1
      ;;
  esac
  tag="${tag_url##*/}"
  tag="${tag%\?*}"
  tag="${tag%/}"
  if [ -z "${tag}" ]; then
    echo "Failed to resolve latest release tag for ${REPO}." >&2
    exit 1
  fi
else
  tag="${VERSION}"
fi

target="${arch_target}-${os_target}"
archive="alchemy-${target}.tar.gz"
url="https://github.com/${REPO}/releases/download/${tag}/${archive}"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

echo "Downloading ${url}"
curl -fsSL "${url}" -o "${tmp_dir}/${archive}"
tar -xzf "${tmp_dir}/${archive}" -C "${tmp_dir}"

binary_path="$(find "${tmp_dir}" -type f -name alchemy | head -n1)"
if [ -z "${binary_path}" ] || [ ! -f "${binary_path}" ]; then
  echo "Downloaded archive does not contain an alchemy binary." >&2
  exit 1
fi

install_to_user_dir() {
  local user_bin_dir="${HOME}/.local/bin"
  mkdir -p "${user_bin_dir}"
  install -m 0755 "${binary_path}" "${user_bin_dir}/alchemy"
  echo "Installed to ${user_bin_dir}/alchemy"
  echo "Ensure ${user_bin_dir} is in your PATH."
}

if [ -w /usr/local/bin ]; then
  install -m 0755 "${binary_path}" /usr/local/bin/alchemy
  echo "Installed to /usr/local/bin/alchemy"
elif command -v sudo >/dev/null 2>&1; then
  sudo install -m 0755 "${binary_path}" /usr/local/bin/alchemy
  echo "Installed to /usr/local/bin/alchemy"
else
  install_to_user_dir
fi

echo "Run: alchemy --help"
