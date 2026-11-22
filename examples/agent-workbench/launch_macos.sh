#!/bin/bash
set -euo pipefail

archive="${RUNFILES_DIR:?Missing Bazel runfiles directory}/$1"
shift
directory=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/xrack-workbench.XXXXXX")
trap '/bin/rm -rf -- "$directory"' EXIT
/usr/bin/ditto -x -k "$archive" "$directory"
app="$directory/XRack Agent Workbench.app"

if [[ ${1:-} == --prepare-only ]]; then
    [[ -d $app ]]
    trap - EXIT
    printf '%s\n' "$app"
    exit 0
fi

has_workspace=no
for arg in "$@"; do
    case "$arg" in
        --workspace|--workspace=*) has_workspace=yes ;;
    esac
done
if [[ $has_workspace == no ]]; then
    set -- --workspace "${BUILD_WORKING_DIRECTORY:-$PWD}" "$@"
fi
/usr/bin/open -W -n -a "$app" --env "PATH=$PATH" --env "CODEX_HOME=${CODEX_HOME:-}" --args "$@"
