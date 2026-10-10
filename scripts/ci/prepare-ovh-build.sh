#!/usr/bin/env bash
set -euo pipefail

if [[ ${RUNNER_ENVIRONMENT:-} != self-hosted || $(id -un) != ferrule-runner ]]; then
  echo 'OVH preparation requires the dedicated ferrule-runner account.' >&2
  exit 1
fi
if [[ ${HOME:?} != /home/ferrule-runner || ! -f ${GITHUB_ENV:?} ]]; then
  echo 'Unexpected runner home or missing GitHub environment file.' >&2
  exit 1
fi
for prerequisite in df awk realpath xvfb-run xauth cc pkg-config; do
  command -v "$prerequisite" >/dev/null
done

# Caches sit outside checkout so checkout's normal clean cannot delete them.
cache_root="$HOME/ci-cache"
if [[ $(realpath -m "$cache_root") != "$cache_root" ||
      $(realpath -m "$cache_root/workspace-target") != "$cache_root/workspace-target" ||
      $(realpath -m "$cache_root/generated-host-target") != "$cache_root/generated-host-target" ]]; then
  echo 'Refusing symlinked build-cache paths.' >&2
  exit 1
fi

available_kib=$(df -Pk "$HOME" | awk 'NR == 2 { print $4 }')
if [[ ! $available_kib =~ ^[0-9]+$ ]] || (( available_kib < 4 * 1024 * 1024 )); then
  echo 'OVH needs at least 4 GiB free before starting a build; inspect Ferrule caches.' >&2
  df -h "$HOME" >&2
  exit 1
fi
mkdir -p "$cache_root/workspace-target" "$cache_root/generated-host-target" "$HOME/.dotnet"

{
  printf 'CARGO_TARGET_DIR=%s/workspace-target\n' "$cache_root"
  printf 'FERRULE_CODEGEN_HOST_TARGET_DIR=%s/generated-host-target\n' "$cache_root"
  printf 'DOTNET_INSTALL_DIR=%s/.dotnet\n' "$HOME"
  printf '%s\n' 'CARGO_BUILD_JOBS=1' 'RUST_TEST_THREADS=2' 'MSBUILDDISABLENODEREUSE=1'
} >> "$GITHUB_ENV"
df -h "$cache_root"
free -h
