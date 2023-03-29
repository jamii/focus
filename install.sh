#! /usr/bin/env bash

nix-shell --run 'zig build install -Doptimize=ReleaseSafe'
pkill -9 focus; sleep 1
cp ./zig-cache/focus-dev ~/bin/focus