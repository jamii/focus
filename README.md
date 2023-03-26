A low-latency text editor.

Probably not useful for anyone but me, but perhaps a useful starting point to fork off your own editor.

To build focus, you'll need `GL`, `X11` development libraries and Zig 0.11.x compiler.

Nix is configured to setup the correct development environment.

Run these commands to build focus:
```
$ nix develop # or nix-shell
$ zig build run -Drelease-safe=true
```

See the architecture notes [here](https://scattered-thoughts.net/#focus).
