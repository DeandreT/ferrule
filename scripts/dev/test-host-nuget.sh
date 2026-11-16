#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "Usage: $0 <nupkg> <package-version> [source-repository-url]" >&2
  exit 2
fi

package="$(realpath -- "$1")"
version="$2"
repository="${3:-https://github.com/DeandreT/ferrule}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$root/crates/host/tests/fixtures/versioned-ledger.json"
scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT

python3 - "$package" "$version" "$repository" "$(git -C "$root" rev-parse HEAD)" "$root/LICENSE" <<'PY'
import pathlib
import sys
import xml.etree.ElementTree as ET
import zipfile

package, version, repository, commit, license_file = sys.argv[1:]
with zipfile.ZipFile(package) as archive:
    manifest = ET.fromstring(archive.read("Ferrule.Host.nuspec"))
    metadata = manifest.find("{*}metadata")
    assert metadata.findtext("{*}id") == "Ferrule.Host"
    assert metadata.findtext("{*}version") == version
    source = metadata.find("{*}repository")
    assert (source.get("type"), source.get("url"), source.get("commit")) == (
        "git", repository, commit
    ), source.attrib
    license_expression = metadata.find("{*}license")
    assert license_expression.attrib["type"] == "expression"
    assert license_expression.text == "GPL-3.0-only"
    assert metadata.findtext("{*}readme") == "README.md"
    assert archive.read("LICENSE") == pathlib.Path(license_file).read_bytes()
    archive.getinfo("README.md")
    archive.getinfo("lib/net10.0/Ferrule.Host.dll")
    native = [name for name in archive.namelist() if name.startswith("runtimes/")]
    assert native == ["runtimes/linux-x64/native/libferrule_host_ffi.so"], native
    with archive.open(native[0]) as library:
        elf = library.read(20)
    assert elf[:6] == b"\x7fELF\x02\x01", "expected a little-endian ELF64 library"
    assert int.from_bytes(elf[16:18], "little") == 3, "expected an ELF shared object"
    assert int.from_bytes(elf[18:20], "little") == 62, "expected an x86-64 library"
print(f"Verified package assets, license, and source commit {commit}.")
PY

mkdir "$scratch/feed" "$scratch/consumer"
cp -- "$package" "$scratch/feed/"
smoke="$root/runtime/csharp/Ferrule.Host.PackageSmoke"
cp -- "$root/global.json" "$smoke/Ferrule.Host.PackageSmoke.csproj" "$smoke/Program.cs" "$scratch/consumer/"
export NUGET_PACKAGES="$scratch/packages"
export DOTNET_CLI_HOME="$scratch/dotnet-home"
unset LD_LIBRARY_PATH LD_PRELOAD DOTNET_ADDITIONAL_DEPS DOTNET_SHARED_STORE
cd "$scratch/consumer"
project=Ferrule.Host.PackageSmoke.csproj
version_property="-p:FerruleHostPackageVersion=$version"

dotnet restore "$project" --source "$scratch/feed" "$version_property" -p:NuGetAudit=false
dotnet build "$project" --no-restore --configuration Release "$version_property"
dotnet bin/Release/net10.0/Ferrule.Host.PackageSmoke.dll "$fixture"

dotnet restore "$project" --source "$scratch/feed" --runtime linux-x64 \
  "$version_property" -p:SelfContained=false -p:NuGetAudit=false
dotnet publish "$project" --no-restore --configuration Release --runtime linux-x64 \
  --self-contained false --output "$scratch/published" "$version_property"
test -f "$scratch/published/libferrule_host_ffi.so"
dotnet "$scratch/published/Ferrule.Host.PackageSmoke.dll" "$fixture"
