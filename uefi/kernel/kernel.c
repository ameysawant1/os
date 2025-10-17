// Simple kernel entry point built as an EFI application for demo purposes
#include <efi.h>
#include <efilib.h>

EFI_STATUS
efi_main(EFI_HANDLE ImageHandle, EFI_SYSTEM_TABLE *SystemTable) {
    InitializeLib(ImageHandle, SystemTable);
    Print(L"Hello from kernel!\n");
    // Halt
    for(;;) {
        uefi_call_wrapper(SystemTable->BootServices->Stall, 1, 1000000);
    }
    return EFI_SUCCESS;
}
