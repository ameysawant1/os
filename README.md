UEFI demo

Dependencies:
- gcc, ld, make
- GNU-EFI headers/libraries (install with: sudo apt install gnu-efi gnu-efi-dev  # Ubuntu/Debian
                                      or: sudo dnf install gnu-efi gnu-efi-devel  # Fedora
                                      or: sudo pacman -S gnu-efi                  # Arch)
- dosfstools (for mkfs.vfat: sudo apt install dosfstools  # Ubuntu/Debian
                           or: sudo pacman -S dosfstools  # Arch)
- qemu-system-x86_64 and OVMF (for testing: sudo apt install qemu-system-x86 ovmf  # Ubuntu/Debian
                                             or: sudo pacman -S qemu ovmf           # Arch)

Build steps:
1. Build bootloader: (cd bootloader && make)
2. Build kernel: (cd ../kernel && make)
3. Build image: (cd ../image && ./build_image.sh)

The resulting image will be in image/output/uefi.img

Test with QEMU:
qemu-system-x86_64 -bios /usr/share/edk2/x64/OVMF.4m.fd -drive file=image/output/uefi.img,format=raw,if=virtio
