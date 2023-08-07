#! /usr/bin/env bash

nix-shell --run 'zig build install -Doptimize=ReleaseSafe'
pkill -9 focus; sleep 1
cp ~/bin/focus "~/bin/focus-$(date +%Y-%m-%d_%H-%M-%S)"
cp ./zig-out/bin/focus-dev ~/bin/focus
cp ./zig-out/bin/focus-dev ~/bin/focus-root