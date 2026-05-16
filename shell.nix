{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.libGL
    # Mesa supplies the actual EGL/GL driver (llvmpipe for software
    # rendering); libGL alone is just libglvnd, the dispatcher.
    pkgs.mesa
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.libGL
    pkgs.mesa
  ];

  # Tell libglvnd where to find Mesa's EGL vendor manifest, and where
  # Mesa's DRI driver shared objects live (llvmpipe ships as a DRI driver
  # used by the surfaceless platform).
  __EGL_VENDOR_LIBRARY_FILENAMES =
    "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
  LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
}
