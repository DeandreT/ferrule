#!/usr/bin/env sh
set -eu

example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$example_dir/../.." && pwd)
generated_dir="$example_dir/generated"

if [ -d "$generated_dir" ]; then
  find "$generated_dir" -depth -mindepth 1 -delete
else
  mkdir -p "$generated_dir"
fi

cargo +nightly run --quiet --manifest-path "$repo_dir/Cargo.toml" -p cli -- generate \
  --project "$example_dir/project.json" \
  --language rust \
  --out "$generated_dir/rust" \
  --rust-runtime-path "$repo_dir/crates/codegen-runtime"

cargo +nightly run --quiet --manifest-path "$example_dir/rust/Cargo.toml"

cargo +nightly run --quiet --manifest-path "$repo_dir/Cargo.toml" -p cli -- generate \
  --project "$example_dir/project.json" \
  --language csharp \
  --out "$generated_dir/csharp"

dotnet run \
  --project "$example_dir/csharp/Ferrule.CodegenExample.csproj" \
  --configuration Release
