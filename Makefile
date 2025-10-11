CC 	    := x86_64-elf-gcc
CFLAGS 	:= -c -m64 -mcmodel=kernel -ffreestanding -nostdlib -mno-red-zone -mno-mmx -mno-sse -mno-sse2 -mno-sse3 -mno-3dnow -Isrc/include -Isrc/include/ares -Isrc/arch/x86_64/include
AS	    := nasm
AFLAGS	:= -felf64
LD	    := x86_64-elf-ld
LFLAGS	:= -nostdlib -z nodefaultlib -z max-page-size=0x1000
RUSTC   := rustc
RUST_TARGET := x86_64-unknown-none
RUSTFLAGS   := -C relocation-model=static -C code-model=kernel -C panic=abort
RUST_SYSROOT := $(shell $(RUSTC) --print sysroot)
RUST_LIBDIR  := $(RUST_SYSROOT)/lib/rustlib/$(RUST_TARGET)/lib
RUST_RLIBS   := $(wildcard $(RUST_LIBDIR)/libcore-*.rlib) \
                 $(wildcard $(RUST_LIBDIR)/libcompiler_builtins-*.rlib)

boot_source_files     := $(shell find src/boot -name "*.asm")
boot_asm_object_files := $(patsubst src/boot/%.asm, build/boot/%.o, $(boot_source_files))

boot_object_files     := $(boot_asm_object_files)

kernel_source_files   := $(shell find src/kernel -name "*.rs" 2>/dev/null)
kernel_object_files   := $(patsubst src/kernel/%.rs, build/kernel/%.o, $(kernel_source_files))

.PHONY: build-x86_64

all: build-x86_64

$(boot_asm_object_files): build/boot/%.o : src/boot/%.asm
	mkdir -p $(dir $@) && \
	$(AS) $(AFLAGS) $(patsubst build/boot/%.o, src/boot/%.asm, $@) -o $@

$(kernel_object_files): build/kernel/%.o : src/kernel/%.rs
	mkdir -p $(dir $@) && \
	$(RUSTC) $(RUSTFLAGS) --target $(RUST_TARGET) --emit=obj -o $@ --crate-type=lib $<

build-x86_64: $(boot_object_files) $(kernel_object_files)
	mkdir -p dist/x86_64 && \
	$(LD) $(LFLAGS) -o dist/x86_64/kernel.bin -T targets/x86_64/linker.ld $(boot_object_files) $(kernel_object_files) $(x86_64_object_files) $(RUST_RLIBS) && \
	cp dist/x86_64/kernel.bin targets/x86_64/iso/boot/kernel.bin && \
	grub-mkrescue /usr/lib/grub/i386-pc -o dist/x86_64/kernel.iso targets/x86_64/iso

clean:
	rm -Rf ./dist
	rm -Rf ./build

run: build-x86_64
	qemu-system-x86_64 -cdrom dist/x86_64/kernel.iso \
										 -serial mon:stdio \
										 -serial file:kernel.log \
										 -d int,cpu_reset \
										 -no-reboot
