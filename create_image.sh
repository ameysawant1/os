#!/bin/bash

# Create a basic disk image for the OS
# This is a simplified version - in a real implementation, you'd use proper UEFI tooling

set -e

IMAGE_SIZE_MB=64
IMAGE_FILE="image/os.img"

echo "Creating OS disk image..."

# Create image directory if it doesn't exist
mkdir -p image

# Create a raw disk image
dd if=/dev/zero of="$IMAGE_FILE" bs=1M count="$IMAGE_SIZE_MB" status=none

# Create a GPT partition table (simplified)
# In a real implementation, you'd use parted or similar tools
echo "Disk image created: $IMAGE_FILE"

# Copy the kernel binary to the image
# This is a placeholder - UEFI would typically load from ESP
if [ -f "target/x86_64-unknown-uefi/release/os.efi" ]; then
    echo "Kernel binary found, but not copying to image (UEFI boot)"
else
    echo "Warning: Kernel binary not found"
fi

echo "Image creation complete."
