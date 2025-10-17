/* Minimal linker script for EFI application */
OUTPUT_FORMAT("elf64-x86-64")
ENTRY(efi_main)
SECTIONS
{
  . = 0x100000;
  .text : { *(.text) }
  .rodata : { *(.rodata) }
  .data : { *(.data) }
  .bss  : { *(.bss) }
}
