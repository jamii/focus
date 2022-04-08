{ cross ? false }:

let

  nixpkgs = builtins.fetchTarball {
      name = "nixos-21.11";
      url = "https://github.com/NixOS/nixpkgs/archive/refs/tags/21.11.tar.gz";
      sha256 = "162dywda2dvfj1248afxc45kcrg83appjd0nmdb541hl7rnncf02";
  };

  hostPkgs = import nixpkgs {};

  armPkgs = import nixpkgs {
    system = "aarch64-linux";
  };

  crossPkgs = import nixpkgs {
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
            if (hostPkgs.system == "x86_64-linux") then {
                url = "https://ziglang.org/download/0.9.0/zig-linux-x86_64-0.9.0.tar.xz";
                sha256 = "1vagp72wxn6i9qscji6k3a1shy76jg4d6crmx9ijpch9kyn71c96";
            } else if (hostPkgs.system == "aarch64-linux") then {
                url = "https://ziglang.org/download/0.9.0/zig-linux-aarch64-0.9.0.tar.xz";
                sha256 = "00m6nxp64nf6pwq407by52l8i0f2m4mw6hj17jbjdjd267b6sgri";
            } else 
                throw ("Unknown system " ++ hostPkgs.system)
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
