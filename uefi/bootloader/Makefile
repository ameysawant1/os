
CC = gcc
LD = ld
OBJCOPY = objcopy
RM = rm -f

EFI_INCLUDES = -I/usr/include -I/usr/include/efi -I/usr/include/efi/x86_64
CFLAGS = -fno-exceptions -fPIC -fno-asynchronous-unwind-tables $(EFI_INCLUDES)
LDFLAGS = -nostdlib -znocombreloc -T /usr/lib/elf_x86_64_efi.lds -shared -Bsymbolic -L/usr/lib -Wl,--no-relax

TARGET = boot.efi
SRCS = boot.c
OBJS = $(SRCS:.c=.o)

all: $(TARGET)

%.o: %.c
	$(CC) -c $< -o $@ $(CFLAGS)

boot.so: $(OBJS)
	$(CC) $(LDFLAGS) -o $@ $(OBJS)

$(TARGET): boot.so
	$(OBJCOPY) -j .text -j .sdata -j .data -j .dynamic -j .dynsym -j .rel -j .rela -j .reloc --target=efi-app-x86_64 boot.so $@

clean:
	$(RM) $(OBJS) boot.so $(TARGET)
