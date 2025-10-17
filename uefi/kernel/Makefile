CC = gcc
LD = ld
RM = rm -f

SRCS = kernel.c
OBJS = $(SRCS:.c=.o)
TARGET = kernel.efi

CFLAGS = -fPIC -fno-exceptions -I/usr/include -I/usr/include/efi -I/usr/include/efi/x86_64
LDFLAGS = -nostdlib -znocombreloc -T linker.ld -shared -Bsymbolic

all: $(TARGET)

%.o: %.c
	$(CC) -c $< -o $@ $(CFLAGS)

$(TARGET): $(OBJS)
	$(LD) $(LDFLAGS) -o $@ $(OBJS)

clean:
	$(RM) $(OBJS) $(TARGET)
