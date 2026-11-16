#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "Usage: $0 <output-directory> <package-version> [source-repository-url]" >&2
  exit 2
fi

output="$(realpath -m -- "$1")"
version="$2"
repository="${3:-https://github.com/DeandreT/ferrule}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
revision="$(git rev-parse HEAD)"
if [[ -n "$(git status --porcelain)" ]]; then
  echo "A clean checkout is required to record the package's exact corresponding source." >&2
  exit 1
fi

target=x86_64-unknown-linux-gnu
target_dir="$(realpath -m -- "${CARGO_TARGET_DIR:-$root/target}")"
scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
mkdir -p "$output"

cargo build --locked --release -p ferrule-host-ffi --target "$target" --target-dir "$target_dir"
dotnet pack runtime/csharp/Ferrule.Host --configuration Release \
  --artifacts-path "$scratch/managed" --output "$output" \
  -p:PackageVersion="$version" \
  -p:RepositoryUrl="$repository" -p:RepositoryCommit="$revision" \
  -p:FerruleNativeLibrary="$target_dir/$target/release/libferrule_host_ffi.so"
