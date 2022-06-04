{ cross ? false }:

let

  hostPkgs = import <nixpkgs> {};

  armPkgs = import <nixpkgs> {
    system = "aarch64-linux";
  };

  crossPkgs = import <nixpkgs> {
    overlays = [(self: super: {
      inherit (armPkgs)
        gcc
        mesa
        libGL
        SDL2
        SDL_ttf
      ;
    })];
    crossSystem = hostPkgs.lib.systems.examples.aarch64-multiplatform;
  };

  targetPkgs = if cross then crossPkgs else hostPkgs;

  zig = hostPkgs.stdenv.mkDerivation {
        name = "zig";
        src = fetchTarball (
            if (targetPkgs.system == "x86_64-linux") then {
                url = "https://ziglang.org/builds/zig-linux-x86_64-0.10.0-dev.2473+e498fb155.tar.xz";
                sha256 = "1iih9wcr5v2k2v384ljv4nalfzgy0kbb0ilz7jdn5gh4h9jhy086";
            } else 
            throw ("Unknown system " ++ targetPkgs.system)
        );
        dontConfigure = true;
        dontBuild = true;
        installPhase = ''
            mkdir -p $out
            mv ./* $out/
            mkdir -p $out/bin
            mv $out/zig $out/bin
        '';
    };

in

hostPkgs.mkShell rec {
  buildInputs = [
    zig
    hostPkgs.pkg-config
    hostPkgs.git
    targetPkgs.libGL.all
    targetPkgs.xorg.libX11.dev
    targetPkgs.xorg.xorgproto
    targetPkgs.SDL2.all
    targetPkgs.SDL2_ttf.all
    targetPkgs.pcre2.all
  ];
  FOCUS="nixos@192.168.1.83";
  NIX_GCC=targetPkgs.gcc;
  NIX_LIBGL_DEV=targetPkgs.libGL.dev;
  NIX_LIBX11_DEV=targetPkgs.xorg.libX11.dev;
  NIX_XORGPROTO_DEV=targetPkgs.xorg.xorgproto;
  NIX_SDL2_DEV=targetPkgs.SDL2.dev;
  NIX_SDL2_TTF_DEV=targetPkgs.SDL2_ttf; # no .dev
  # TODO with SDL_VIDEODRIVER=wayland, SDL doesn't seem to respect xkb settings eg caps vs ctrl
  # but without, sometimes causes https://github.com/swaywm/sway/issues/5227
  # SDL_VIDEODRIVER="wayland";
  NIX_PCRE2_DEV = targetPkgs.pcre2.dev;
}
