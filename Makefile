all: build

build:
	cargo build --release --target x86_64-unknown-uefi
	mkdir -p image
	cp target/x86_64-unknown-uefi/release/os.efi image/BOOTX64.EFI
	./create_image.sh

clean:
	cargo clean
	rm -rf image target

run: build
	qemu-system-x86_64 -bios /usr/share/edk2/x64/OVMF.4m.fd \
		-drive file=image/os.img,format=raw,if=virtio \
		-nographic

.PHONY: build clean run