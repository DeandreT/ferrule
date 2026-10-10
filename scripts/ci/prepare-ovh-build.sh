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
for prerequisite in df awk realpath xvfb-run xauth cc pkg-config sccache; do
  command -v "$prerequisite" >/dev/null
done
timeout_path=$(command -v gnutimeout || command -v timeout)
timeout_path=$(realpath "$timeout_path")
if [[ ! -f $timeout_path || -L $timeout_path ]] ||
   [[ $("$timeout_path" --version) != 'timeout (GNU coreutils)'* ]]; then
  echo 'OVH codegen qualification requires a regular GNU timeout executable.' >&2
  exit 1
fi

# Caches sit outside checkout so checkout's normal clean cannot delete them.
cache_root="$HOME/ci-cache"
if [[ $(realpath -m "$cache_root") != "$cache_root" ||
      $(realpath -m "$cache_root/workspace-target") != "$cache_root/workspace-target" ||
      $(realpath -m "$cache_root/generated-host-target") != "$cache_root/generated-host-target" ||
      $(realpath -m "$cache_root/compiler") != "$cache_root/compiler" ||
      $(realpath -m "$HOME/ci-tmp") != "$HOME/ci-tmp" ]]; then
  echo 'Refusing symlinked build-cache paths.' >&2
  exit 1
fi

available_kib=$(df -Pk "$HOME" | awk 'NR == 2 { print $4 }')
case ${FERRULE_CI_SUITE:-native} in
  native) reserve_gib=16 ;;
  codegen) reserve_gib=8 ;;
  *) echo 'Unknown Ferrule CI resource profile.' >&2; exit 1 ;;
esac
if [[ ! $available_kib =~ ^[0-9]+$ ]] || (( available_kib < reserve_gib * 1024 * 1024 )); then
  printf 'OVH needs at least %s GiB free before starting this build; inspect Ferrule caches.\n' "$reserve_gib" >&2
  df -h "$HOME" >&2
  exit 1
fi
mkdir -p "$cache_root/workspace-target" "$cache_root/generated-host-target" \
  "$cache_root/compiler" "$HOME/.dotnet" "$HOME/ci-tmp"

{
  printf 'CARGO_TARGET_DIR=%s/workspace-target\n' "$cache_root"
  printf 'FERRULE_CODEGEN_HOST_TARGET_DIR=%s/generated-host-target\n' "$cache_root"
  printf 'DOTNET_INSTALL_DIR=%s/.dotnet\n' "$HOME"
  printf 'TMPDIR=%s/ci-tmp\n' "$HOME"
  printf 'FERRULE_CODEGEN_GNU_TIMEOUT=%s\n' "$timeout_path"
  printf 'SCCACHE_DIR=%s/compiler\n' "$cache_root"
  printf '%s\n' 'SCCACHE_CACHE_SIZE=2G' 'CARGO_BUILD_JOBS=1' 'RUST_TEST_THREADS=2' \
    'MSBUILDDISABLENODEREUSE=1' 'UseSharedCompilation=false'
} >> "$GITHUB_ENV"
df -h "$cache_root"
free -h
