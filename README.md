A low-latency text editor.

Not intended ot be useful for anyone but me, but perhaps a useful starting point to fork off your own editor.

``` sh
nix develop
zig build run -Doptimize=ReleaseSafe -Dhome-path=/home/jamie -Dprojects-file-path=/home/jamie/secret/projects
```
