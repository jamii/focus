{
  description = "Minimalist text editor written in Zig";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/release-22.05";
    flake-utils.url = "github:numtide/flake-utils";
    zig.url = "github:mitchellh/zig-overlay";

    # Used for shell.nix compatibility
    flake-compat = {
      url = github:edolstra/flake-compat;
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    ...
  } @ inputs: let
    overlays = [
      # Other overlays
      (final: prev: {
        # Replace `master` with a Zig version or a build date to pin package
        # Show available versions using: nix flake show 'github:mitchellh/zig-overlay'
        zigpkg = inputs.zig.packages.${prev.system}.master;
      })
    ];

    # Our supported systems are the same supported systems as the Zig binaries
    systems = builtins.attrNames inputs.zig.packages;
  in
    flake-utils.lib.eachSystem systems (
      system: let
        pkgs = import nixpkgs {inherit overlays system;};

      in rec {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            zigpkg
            libGL.all
            xorg.libX11.dev
          ];
          shellHook = ''
            # Create symlinks in project root
            # HACK: Symlink doesn't get deleted when `nix develop` shell is exitted
            # Zig stdlib - useful for browsing
            if [ ! -L ./zig-stdlib ]; then
              ln -s ${pkgs.zigpkg} ./zig-stdlib
            fi
          '';

        };

        # For compatibility with older versions of the `nix` binary
        devShell = self.devShells.${system}.default;
      }
    );
}
