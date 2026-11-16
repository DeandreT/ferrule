#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
artifacts="$(mktemp -d)"
trap 'rm -rf -- "$artifacts"' EXIT

cargo test --locked -p ferrule-host -p ferrule-host-ffi --target-dir "$target_dir"
cargo build --locked -p ferrule-host-ffi --target-dir "$target_dir"
native="$target_dir/debug/libferrule_host_ffi.so"
fixtures="$root/crates/host/tests/fixtures"
ffi="$root/crates/host-ffi"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror -I"$ffi/include" \
  "$ffi/tests/abi_smoke.c" "$native" -Wl,-rpath,"$(dirname "$native")" -o "$artifacts/abi-smoke"
"$artifacts/abi-smoke" "$fixtures/versioned-ledger.json"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror -fPIC -shared -I"$ffi/include" \
  "$ffi/tests/incompatible_abi.c" -o "$artifacts/libincompatible_abi.so"

dotnet build runtime/csharp/Ferrule.Host.SmokeTests --configuration Release \
  --artifacts-path "$artifacts/dotnet"
smoke="$artifacts/dotnet/bin/Ferrule.Host.SmokeTests/release/Ferrule.Host.SmokeTests.dll"
dotnet "$smoke" "$native" "$fixtures" smoke
dotnet "$smoke" "$artifacts/libincompatible_abi.so" "$fixtures" mismatch
