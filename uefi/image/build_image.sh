#!/usr/bin/env bash
set -euo pipefail

# Build a FAT32 EFI system image containing bootloader and kernel.
OUTDIR=$(pwd)/output
BOOTDIR=${OUTDIR}/EFI/BOOT
mkdir -p "$BOOTDIR"

# copy built EFIs
if [ ! -f ../bootloader/boot.efi ]; then
  echo "bootloader/boot.efi not found. Build it first: (cd ../bootloader && make)" >&2
  exit 1
fi
cp ../bootloader/boot.efi "$BOOTDIR/BOOTX64.EFI"

if [ -f ../kernel/kernel.efi ]; then
  cp ../kernel/kernel.efi "$BOOTDIR/kernel.efi"
fi

# Create a 20MB empty file and format as FAT
IMG=${OUTDIR}/uefi.img
truncate -s 20M "$IMG"
mkfs.vfat -n UEFI "$IMG" >/dev/null 2>&1 || (echo "mkfs.vfat not found" >&2; exit 1)

# Mount, copy files
MOUNTPOINT=$(mktemp -d)
sudo mount -o loop "$IMG" "$MOUNTPOINT"
sudo cp -r "$OUTDIR/EFI" "$MOUNTPOINT/"
sync
sudo umount "$MOUNTPOINT"
rm -rf "$MOUNTPOINT"

echo "Created EFI image at: $IMG"
