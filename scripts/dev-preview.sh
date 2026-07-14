#!/bin/zsh

set -e

repo_root=${0:A:h:h}
cd "$repo_root"

if (( $# > 0 )); then
    cargo run -p xtask -- preview-macos "$@"
    exit
fi

while true; do
    cargo run -p xtask -- preview-macos
done
