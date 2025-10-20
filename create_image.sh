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

# Create a raw disk image
dd if=/dev/zero of="$IMAGE_FILE" bs=1M count="$IMAGE_SIZE_MB" status=none

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

# Determine partition start (bytes) so we can attach just the partition via offset
PART_START_RAW=$(parted -s "$IMAGE_FILE" unit B print | awk '/^ 1/ {print $2; exit}')
if [ -z "$PART_START_RAW" ]; then
    echo "Failed to determine partition start offset" >&2
    exit 1
fi
PART_START_BYTES=${PART_START_RAW%B}

# If mtools (mformat/mcopy) are available we can write to the ESP without root
if false; then  # Disable mtools path for now
    echo "Using mtools to format and populate the ESP (no root required)"
    # mformat expects a geometry or drive letter; use the -i image@@offset syntax
    if ! mformat -i "${IMAGE_FILE}@@${PART_START_BYTES}" :: >/dev/null 2>&1; then
        echo "mformat failed. ESP may be unformatted." >&2
    fi

    if [ -f "target/x86_64-unknown-uefi/release/os.efi" ]; then
        mmd -i "${IMAGE_FILE}@@${PART_START_BYTES}" ::EFI >/dev/null 2>&1 || true
        mmd -i "${IMAGE_FILE}@@${PART_START_BYTES}" ::EFI/BOOT >/dev/null 2>&1 || true
        mcopy -i "${IMAGE_FILE}@@${PART_START_BYTES}" target/x86_64-unknown-uefi/release/os.efi ::EFI/BOOT/BOOTX64.EFI >/dev/null 2>&1 || {
            echo "mcopy failed to copy os.efi into image; ESP may be unformatted." >&2
        }
        echo "Copied os.efi -> EFI/BOOT/BOOTX64.EFI (via mtools)"
    else
        echo "Warning: target/x86_64-unknown-uefi/release/os.efi not found; ESP created but empty." >&2
    fi

else
    # Use root path with mkfs.vfat for better compatibility
    if [ "$(id -u)" -ne 0 ]; then
        echo "To create a proper EFI System Partition, run with sudo:" >&2
        echo "  sudo make build" >&2
        echo "Or install mtools and re-run." >&2
        exit 1
    fi

    echo "Running as root: attaching partition and formatting/mounting to populate ESP"
    LOOP_PART=$(losetup --show -o "$PART_START_BYTES" -f "$IMAGE_FILE")
    if [ -z "$LOOP_PART" ]; then
        echo "Failed to setup loop device for partition" >&2
        exit 1
    fi

    if command -v mkfs.vfat >/dev/null 2>&1; then
        mkfs.vfat -F32 "$LOOP_PART" >/dev/null 2>&1 || {
            echo "mkfs.vfat failed. Cleaning up loop device." >&2
            losetup -d "$LOOP_PART" || true
            exit 1
        }
    else
        echo "mkfs.vfat not found; ESP will be left unformatted" >&2
    fi

    # Mount and copy
    MOUNTDIR=$(mktemp -d)
    mount "$LOOP_PART" "$MOUNTDIR"
    if [ -f "target/x86_64-unknown-uefi/release/os.efi" ]; then
        mkdir -p "$MOUNTDIR/EFI/BOOT"
        cp target/x86_64-unknown-uefi/release/os.efi "$MOUNTDIR/EFI/BOOT/BOOTX64.EFI"
        sync
        echo "Copied os.efi -> EFI/BOOT/BOOTX64.EFI"
    else
        echo "Warning: target/x86_64-unknown-uefi/release/os.efi not found; ESP created but empty." >&2
    fi

    umount "$MOUNTDIR"
    rmdir "$MOUNTDIR"
    losetup -d "$LOOP_PART"
fi

echo "Disk image created: $IMAGE_FILE (with EFI System Partition)"

echo "Image creation complete."
