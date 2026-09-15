#!/usr/bin/env bash
# Shell laid out the way an indenter has to reproduce it. The test types it
# back in with all of the indentation stripped off.
set -euo pipefail

build_everything() {
  local target="$1"
  if [ -z "$target" ]; then
    echo "no target" >&2
    return 1
  elif [ "$target" = all ]; then
    for name in one two three; do
      echo "building $name"
    done
  else
    case "$target" in
      one)
        echo "just one"
        ;;
      *)
        echo "unknown"
        ;;
    esac
  fi
  while read -r line; do
    echo "$line"
  done
}

build_everything "$@"
