#!/usr/bin/env bash
set -euo pipefail

target="${1:-all}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p dist

build_release() {
  cargo build --release --locked
}

case "$target" in
  arch)
    command -v makepkg >/dev/null || { echo "需要 Arch 的 makepkg" >&2; exit 1; }
    build_release
    (cd packaging/arch && makepkg --force --nodeps)
    cp packaging/arch/cipher-vault-*.pkg.tar.* dist/
    ;;
  debian)
    command -v cargo-deb >/dev/null || cargo install cargo-deb --locked
    build_release
    cargo deb --no-build
    cp target/debian/*.deb dist/
    ;;
  fedora)
    command -v cargo-generate-rpm >/dev/null || cargo install cargo-generate-rpm --locked
    build_release
    cargo generate-rpm
    cp target/generate-rpm/*.rpm dist/
    ;;
  windows)
    if [[ "${OS:-}" != "Windows_NT" ]]; then
      echo "MSI 必须在装有 WiX Toolset 的 Windows 环境中生成." >&2
      echo "请在 Windows PowerShell 或 Git Bash 中运行: scripts/package.sh windows" >&2
      exit 1
    fi
    command -v cargo-wix >/dev/null || cargo install cargo-wix --locked
    cargo wix --nocapture
    cp target/wix/*.msi dist/
    ;;
  all)
    "$0" arch
    "$0" debian
    "$0" fedora
    "$0" windows
    ;;
  *)
    echo "用法: $0 {arch|debian|fedora|windows|all}" >&2
    exit 2
    ;;
esac

echo "安装包已生成到 $root/dist"
