#!/bin/bash

# Create a UEFI-compatible disk image for the OS

set -e

IMAGE_SIZE_MB=64
IMAGE_FILE="image/os.img"
# ESP parameters (in MiB)
ESP_START_MB=1
ESP_SIZE_MB=32

echo "Creating OS disk image..."

# Create image directory if it doesn't exist
mkdir -p image

# Ensure ESP size fits inside image
if [ "$ESP_SIZE_MB" -ge "$IMAGE_SIZE_MB" ]; then
    echo "ESP size ($ESP_SIZE_MB MiB) must be smaller than total image size ($IMAGE_SIZE_MB MiB)" >&2
    exit 1
fi

echo "Creating GPT and EFI System Partition (ESP) on $IMAGE_FILE"

# Create partition table and EFI partition using parted
parted -s "$IMAGE_FILE" mklabel gpt || {
    echo "parted not available or failed. The image will be raw without ESP." >&2
    echo "Disk image created: $IMAGE_FILE"
    exit 0
}

# Calculate end offset for parted in MiB
ESP_END_MB=$((ESP_START_MB + ESP_SIZE_MB))
parted -s "$IMAGE_FILE" mkpart ESP fat32 ${ESP_START_MB}MiB ${ESP_END_MB}MiB
parted -s "$IMAGE_FILE" set 1 boot on

# Create a loop device for the partition
LOOPDEV=$(losetup --show -fP "$IMAGE_FILE")
if [ -z "$LOOPDEV" ]; then
    echo "Failed to setup loop device for $IMAGE_FILE" >&2
    exit 1
fi

ESP_DEV="${LOOPDEV}p1"
if [ ! -b "$ESP_DEV" ]; then
    # Some systems expose partitions as ${LOOPDEV}p1, others as ${LOOPDEV}1
    ESP_DEV="${LOOPDEV}1"
fi

# Try mkfs.vfat first, then mformat (from mtools), otherwise warn and continue
if command -v mkfs.vfat >/dev/null 2>&1; then
    mkfs.vfat -F32 "$ESP_DEV" >/dev/null 2>&1 || {
        echo "mkfs.vfat failed. Cleaning up loop device." >&2
        losetup -d "$LOOPDEV" || true
        exit 1
    }
elif command -v mformat >/dev/null 2>&1 && command -v mcopy >/dev/null 2>&1; then
    # mtools can operate directly on the image but needs an offset; use mformat on the loop device partition
    mformat -i "$ESP_DEV" :: || {
        echo "mformat failed. Cleaning up loop device." >&2
        losetup -d "$LOOPDEV" || true
        exit 1
    }
else
    echo "Warning: neither mkfs.vfat nor mtools found. ESP created but not formatted." >&2
fi

# Mount the ESP and copy the EFI binary
MOUNTDIR=$(mktemp -d)
mount "$ESP_DEV" "$MOUNTDIR"

if [ -f "target/x86_64-unknown-uefi/release/os.efi" ]; then
    mkdir -p "$MOUNTDIR/EFI/BOOT"
    # Copy kernel as default bootloader (BOOTX64.EFI)
    cp target/x86_64-unknown-uefi/release/os.efi "$MOUNTDIR/EFI/BOOT/BOOTX64.EFI"
    sync
    echo "Copied os.efi -> EFI/BOOT/BOOTX64.EFI"
else
    echo "Warning: target/x86_64-unknown-uefi/release/os.efi not found; ESP created but empty." >&2
fi

umount "$MOUNTDIR"
rmdir "$MOUNTDIR"
losetup -d "$LOOPDEV"

echo "Disk image created: $IMAGE_FILE (with EFI System Partition)"

echo "Image creation complete."
if [ -f "target/x86_64-unknown-uefi/release/os.efi" ]; then
    mmd -i "$IMAGE_FILE"@@$ESP_OFFSET ::EFI
    mmd -i "$IMAGE_FILE"@@$ESP_OFFSET ::EFI/BOOT
    mcopy -i "$IMAGE_FILE"@@$ESP_OFFSET target/x86_64-unknown-uefi/release/os.efi ::EFI/BOOT/BOOTX64.EFI
    echo "EFI binary copied to ESP"
else
    echo "Warning: Kernel binary not found at target/x86_64-unknown-uefi/release/os.efi"
fi

echo "Image creation complete: $IMAGE_FILE"
