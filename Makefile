all: build

build:
	cargo build --release --target x86_64-unknown-uefi
	mkdir -p image
	./create_image.sh

clean:
	cargo clean
	rm -rf image target

run: build
	qemu-system-x86_64 -bios /usr/share/edk2/x64/OVMF.4m.fd \
		-drive file=image/os.img,format=raw,if=virtio \
		-serial mon:stdio \
		-monitor none \
		-boot order=c

run-text: build
	qemu-system-x86_64 -bios /usr/share/edk2/x64/OVMF.4m.fd \
		-drive file=image/os.img,format=raw,if=virtio \
		-serial mon:stdio \
		-monitor none \
		-nographic \
		-boot order=c

.PHONY: build clean run