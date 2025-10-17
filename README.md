# Rust UEFI OS

A simple operating system written in Rust that boots via UEFI.

## Dependencies

- Rust toolchain (with `x86_64-unknown-uefi` target)
- GNU-EFI development libraries (for image creation)
- dosfstools (for mkfs.vfat)
- QEMU with OVMF (for testing)

Install dependencies:

```bash
# Install Rust if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add x86_64-unknown-uefi

# Install system dependencies (Ubuntu/Debian)
sudo apt install dosfstools qemu-system-x86 ovmf

# Install system dependencies (Arch Linux)
sudo pacman -S dosfstools qemu ovmf
```

## Build

```bash
make build
```

This will:

1. Compile the Rust UEFI application
2. Create a FAT32 EFI system image
3. Copy the EFI binary to the correct location

## Test

```bash
make run
```

Or manually:

```bash
qemu-system-x86_64 -bios /usr/share/edk2/x64/OVMF.4m.fd \
  -drive file=image/os.img,format=raw,if=virtio \
  -nographic
```

## Project Structure

- `src/main.rs` - Main UEFI application (bootloader + kernel)
- `Cargo.toml` - Rust dependencies and build configuration
- `Makefile` - Build automation
- `create_image.sh` - EFI image creation script
- `image/` - Generated EFI image and binaries
