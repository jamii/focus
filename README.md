A low-latency text editor.

Probably not useful for anyone but me, but perhaps a useful starting point to fork off your own editor.

Unlikely to build out of the box on anything that isn't my laptop:

```
nix-shell
zig build run -Drelease-safe=true
```

See the architecture notes [here](https://scattered-thoughts.net/writing/focus-intro/).
