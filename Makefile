# arch
default:
	NIX_SDL2_TTF_DEV=/usr/include/SDL2 NIX_SDL2_DEV=/usr/include/SDL2 NIX_LIBX11_DEV=/usr/include/X11 NIX_XORGPROTO_DEV=/usr/include/xorg NIX_LIBGL_DEV=/usr/include/GL NIX_PCRE2_DEV=/usr/include zig build run -Drelease-safe=true
