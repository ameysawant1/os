// Simple UEFI bootloader that loads a kernel EFI stub or jumps to kernel
#include <efi.h>
#include <efilib.h>

EFI_STATUS
efi_main(EFI_HANDLE ImageHandle, EFI_SYSTEM_TABLE *SystemTable) {
    InitializeLib(ImageHandle, SystemTable);
    Print(L"Hello UEFI World from bootloader!\n");

    // In a real bootloader we'd load the kernel image here. For demo
    // purposes we just wait for a key press then exit.
    Print(L"Press any key to exit...\n");
    EFI_INPUT_KEY Key;
    UINTN EventIndex;
    SystemTable->ConIn->Reset(SystemTable->ConIn, FALSE);
    uefi_call_wrapper(SystemTable->BootServices->WaitForEvent, 2, 1, &SystemTable->ConIn->WaitForKey, &EventIndex);
    SystemTable->ConIn->ReadKeyStroke(SystemTable->ConIn, &Key);

    return EFI_SUCCESS;
}
