#! /usr/bin/env bash

set -euo pipefail

nix-shell --run 'cargo build --release'

pkill -9 focus || true
sleep 1

cp ./target/release/focus ~/bin/focus
