#!/bin/bash
set -e

echo "Creating UEFI boot image..."

# Create EFI directory structure
mkdir -p image/EFI/BOOT

# Copy the EFI binary to the standard location
cp target/x86_64-unknown-uefi/release/os.efi image/EFI/BOOT/BOOTX64.EFI

# Create a FAT32 image
dd if=/dev/zero of=image/os.img bs=1M count=20
mkfs.vfat -n UEFI_OS image/os.img >/dev/null 2>&1

# Mount and copy files
MOUNTPOINT=$(mktemp -d)
sudo mount -o loop image/os.img "$MOUNTPOINT"
sudo cp -r image/EFI "$MOUNTPOINT/"
sync
sudo umount "$MOUNTPOINT"
rmdir "$MOUNTPOINT"

echo "UEFI image created at image/os.img"