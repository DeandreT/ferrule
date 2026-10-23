#!/usr/bin/env bash
# Stage a private PDF worker package from a Cargo build.
#
# Cargo hard-links every built binary between target/<profile>/ and
# target/<profile>/deps/, and packaged-worker admission requires a single-link
# worker executable, so `ferrule --pdf-worker` and the GUI worker option refuse
# to start from the build directory. This copies the front ends and the worker
# into one folder that the unchanged installed-package admission accepts. It
# never links, and it never relaxes or bypasses an admission check.
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/dev/stage-pdf-worker-package.sh [--release] [--cli-only]
                                               [--build-dir DIR] [--dest DIR]

Builds ferrule, ferrule-pdf-worker and, unless --cli-only, ferrule-gui with
`cargo build --locked`, then copies them into DEST. Run the copies from DEST.

  --release        build and stage the release profile
  --cli-only       stage only ferrule and ferrule-pdf-worker
  --build-dir DIR  stage the binaries already built in DIR; do not build
  --dest DIR       package folder to create (mode 0700) or refresh;
                   default: <build directory>/pdf-worker-package

DEST and every folder above it must be owned by you or root and must not be
writable by group or others, except a root-owned sticky folder such as /tmp.
USAGE
}

die() {
    printf 'stage-pdf-worker-package: %s\n' "$*" >&2
    exit 1
}

release=false
cli_only=false
build_dir=
dest=
while (($#)); do
    case $1 in
        --release) release=true ;;
        --cli-only) cli_only=true ;;
        --build-dir | --dest)
            (($# >= 2)) || die "$1 needs a value"
            if [[ $1 == --build-dir ]]; then build_dir=$2; else dest=$2; fi
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *) die "unknown argument $1 (see --help)" ;;
    esac
    shift
done

names=(ferrule ferrule-pdf-worker)
packages=(-p cli)
if ! $cli_only; then
    names+=(ferrule-gui)
    packages+=(-p gui)
fi

if [[ -z $build_dir ]]; then
    repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
    profile=()
    if $release; then profile=(--release); fi
    # Cargo reports each binary's uplifted path, whatever its target directory.
    built=$(cd -- "$repo" && "${CARGO:-cargo}" build --locked "${profile[@]}" "${packages[@]}" \
        --bins --message-format=json-render-diagnostics)
    cli=$(sed -n 's/.*"executable":"\([^"]*\/ferrule\)".*/\1/p' <<<"$built")
    [[ -n $cli ]] || die "cargo did not report the ferrule executable"
    build_dir=$(dirname -- "$cli")
elif $release; then
    die "--release selects a profile to build; it cannot be combined with --build-dir"
fi

for name in "${names[@]}"; do
    if [[ ! -f $build_dir/$name || ! -x $build_dir/$name ]]; then
        hint=
        if [[ $name == ferrule-gui ]]; then hint='; build it or pass --cli-only'; fi
        die "$build_dir/$name is not a built executable$hint"
    fi
done

dest=${dest:-$build_dir/pdf-worker-package}
parent=$(realpath -e -- "$(dirname -- "$dest")") || die "the parent of $dest does not exist"
dest=$parent/$(basename -- "$dest")

# Mirrors the admission walk so an unusable folder fails here, before any copy.
# Admission itself stays authoritative when the staged programs run.
uid=$(id -u)
check=$parent
while [[ $check != / ]]; do
    read -r owner mode < <(stat -c '%u %a' -- "$check")
    mode=$((8#$mode))
    if { ((owner != 0 && owner != uid)); } ||
        { ((mode & 8#022)) && ! ((owner == 0 && mode & 8#1000)); }; then
        die "worker admission refuses $check: it must be owned by you or root and not writable by group or others"
    fi
    check=$(dirname -- "$check")
done

if [[ -L $dest || (-e $dest && ! -d $dest) ]]; then
    die "$dest exists and is not a directory"
elif [[ -d $dest ]]; then
    read -r owner mode < <(stat -c '%u %a' -- "$dest")
    ((owner == uid && !((8#$mode) & 8#022))) ||
        die "$dest must be owned by you and not writable by group or others"
else
    mkdir -m 0700 -- "$dest"
fi

staging=
trap '[[ -z $staging ]] || rm -f -- "$staging"' EXIT
for name in "${names[@]}"; do
    # Copy into a fresh private file, then rename it over the previous copy:
    # the staged program is always a new single-link file, and a copy that is
    # still running keeps its own executable.
    staging=$(mktemp -- "$dest/.$name.XXXXXX")
    cp -- "$build_dir/$name" "$staging"
    chmod 0700 -- "$staging"
    mv -fT -- "$staging" "$dest/$name"
    staging=
    [[ $(stat -c %h -- "$dest/$name") == 1 ]] || die "$dest/$name is not a single-link copy"
done

printf 'Staged %s in %s\n' "${names[*]}" "$dest"
printf 'Run them from there, for example:\n  %s/ferrule run --project PROJECT --pdf-worker\n' "$dest"
if ! $cli_only; then
    printf '  %s/ferrule-gui\n' "$dest"
fi
