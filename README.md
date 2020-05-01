Status:

* mobile-nixos installed but no config.nix yet
* host build works
* cross build works
* target build is broken - compiler crashes
* how to setup ssh-over-usb?

* memory
  * urgency seems broken
  * no touch yet
  * cpu usage is high

# Installing mobile-nixos:

https://mobile.nixos.org/getting-started.html

On local:

```
# base image
pushd mobile-nixos
nix-build --argstr device pine64-pinephone-braveheart -A build.disk-image
dd if=result of=/dev/mmcblk0 bs=8M oflag=sync,direct status=progress
popd

# resize root partition
sudo fdisk /dev/mmcblk0
# d 2, n 2

# bootable root
# download latest from https://hydra.nixos.org/job/mobile-nixos/unstable/examples-demo.aarch64-linux.rootfs
unzstd ~/Downloads/NIXOS_SYSTEM.img.zst
dd if=Downloads/NIXOS_SYSTEM.img of=/dev/mmcblk0p2 bs=8M oflag=sync,direct status=progress

# boot focus
# connect to wifi
```

On focus:

```
# password is nixos
# set authorized_keys
sudo date --set="11 APR 2020 14:02:00"
sudo nix-channel --update
```

Uses config from https://github.com/NixOS/mobile-nixos/blob/master/examples/demo/configuration.nix

Local build:

```
nix-shell
zig build run
```

Cross build:

```
nix-shell --arg cross true
zig build cross
./sync
ssh $FOCUS
  cd /home/jamie
  nix-shell
  export DISPLAY=:0
  export SDL_VIDEODRIVER=wayland
  ./focus
```

# TODO

* functions
  * keyboard
  * menu
  * alarm, timer
  * sms
  * calls
  * contacts
  * calendar
  * music
  * pdf/epub
  * maps
  * notes
  * weather
  * bank
  * photo/video
  * otp
  * email
  * matrix
  * sync
  * file upload
* support
  * layout
  * font
  * anti-aliasing
  * touch input
  * gestures
  * scroll
