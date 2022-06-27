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
    hostPkgs.git
    targetPkgs.libGL.all
    targetPkgs.xorg.libX11.dev
    targetPkgs.pcre2.all
  ];
}
