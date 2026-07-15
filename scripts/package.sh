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
    cp packaging/arch/helm-*.pkg.tar.* dist/
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
      echo "MSI 必须在装有新版 WiX Toolset 的 Windows 环境中生成." >&2
      echo "请在 Windows PowerShell 或 Git Bash 中运行: scripts/package.sh windows" >&2
      exit 1
    fi
    command -v wix >/dev/null || {
      echo "找不到 wix.exe. 请运行: dotnet tool install --global wix" >&2
      exit 1
    }
    command -v cygpath >/dev/null || {
      echo "Windows 打包脚本需要在 Git Bash 中运行." >&2
      exit 1
    }
    build_release
    version="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"
    binary="$(cygpath -w "$root/target/release/helm.exe")"
    icon="$(cygpath -w "$root/assets/icons/helm.ico")"
    output="$(cygpath -w "$root/dist/helm-$version-x86_64.msi")"
    wix build \
      -arch x64 \
      -d "AppBinary=$binary" \
      -d "IconFile=$icon" \
      -d "Version=$version" \
      -out "$output" \
      wix/main.wxs
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
