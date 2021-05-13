#! /usr/bin/env bash

nix-shell --run 'zig build local -Drelease-safe=true'
pkill -9 focus-stable; sleep 1
cp ./zig-cache/focus-local ./zig-cache/focus-stable