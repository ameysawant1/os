all: uefi

.PHONY: uefi
uefi:
	$(MAKE) -C uefi/bootloader all
	$(MAKE) -C uefi/kernel all
	(cd uefi/image && ./build_image.sh)

clean:
	rm -rf uefi/*/build uefi/*/*.o uefi/*/*.so uefi/*/*.efi uefi/image/output
uefi:
	$(MAKE) -C uefi/bootloader all
	$(MAKE) -C uefi/kernel all
	(cd uefi/image && ./build_image.sh)