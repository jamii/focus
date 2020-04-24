Status:

* mobile-nixos installed but no config.nix yet
* local build is broken (X BadWindow) but worker earlier
* cross build seems to work but need to copy over .so
* focus build works
* why is drag laggy?
  * seems like input lag - turning off vsync fixes it but event polling loop is at 100%
  * can put a sleep in the loop to fix it
  * how best to sample input?
* how to setup ssh-over-usb?

* memory
  * urgency seems broken
  * no draw yet

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
# connect to network
```

On focus:

```
# password is nixos
# set authorized_keys
sudo date --set="11 APR 2020 14:02:00"
sudo nix-channel --update
```

Uses config from https://github.com/NixOS/mobile-nixos/blob/master/examples/demo/configuration.nix
