CC 	    := x86_64-elf-gcc
CFLAGS 	:= -c -m64 -mcmodel=kernel -ffreestanding -nostdlib -mno-red-zone -mno-mmx -mno-sse -mno-sse2 -mno-sse3 -mno-3dnow -Isrc/include -Isrc/include/ares -Isrc/arch/x86_64/include
AS	    := nasm
AFLAGS	:= -felf64
LD	    := x86_64-elf-ld
LFLAGS	:= -nostdlib -z nodefaultlib -z max-page-size=0x1000
RUSTC   := rustc

RUST_TARGET 	:= x86_64-unknown-none
RUSTFLAGS   	:= -C relocation-model=static -C code-model=kernel -C panic=abort
RUST_SYSROOT 	:= $(shell $(RUSTC) --print sysroot)
RUST_LIBDIR  	:= $(RUST_SYSROOT)/lib/rustlib/$(RUST_TARGET)/lib
RUST_RLIBS   	:= $(wildcard $(RUST_LIBDIR)/libcore-*.rlib) \
                 $(wildcard $(RUST_LIBDIR)/liballoc-*.rlib) \
                 $(wildcard $(RUST_LIBDIR)/libcompiler_builtins-*.rlib) \
                 $(wildcard $(RUST_LIBDIR)/librustc_std_workspace_core-*.rlib)

boot_source_dir       := src/arch/x86_64/boot
boot_build_dir        := build/x86_64/boot
boot_source_files     := $(shell find $(boot_source_dir) -name "*.asm")
boot_asm_object_files := $(patsubst $(boot_source_dir)/%.asm, $(boot_build_dir)/%.o, $(boot_source_files))

boot_object_files     := $(boot_asm_object_files)

kernel_source_files   := src/kernel/kmain.rs
kernel_object_files   := build/kernel/kmain.o

arch_kernel_source_dir        := src/arch/x86_64/kernel
arch_kernel_build_dir         := build/arch/x86_64/kernel
arch_kernel_asm_source_files  := $(shell find $(arch_kernel_source_dir) -name "*.asm" 2>/dev/null)
arch_kernel_asm_object_files  := $(patsubst $(arch_kernel_source_dir)/%.asm, $(arch_kernel_build_dir)/%.o, $(arch_kernel_asm_source_files))

arch_kernel_object_files      := $(arch_kernel_asm_object_files)

.PHONY: build-x86_64

all: build-x86_64

$(boot_asm_object_files): $(boot_build_dir)/%.o : $(boot_source_dir)/%.asm
	mkdir -p $(dir $@) && \
	$(AS) $(AFLAGS) $(patsubst $(boot_build_dir)/%.o, $(boot_source_dir)/%.asm, $@) -o $@

$(kernel_object_files): build/kernel/%.o : src/kernel/%.rs
	mkdir -p $(dir $@) && \
	$(RUSTC) $(RUSTFLAGS) --target $(RUST_TARGET) --emit=obj -o $@ --crate-type=lib $<

$(arch_kernel_asm_object_files): $(arch_kernel_build_dir)/%.o : $(arch_kernel_source_dir)/%.asm
	mkdir -p $(dir $@) && \
	$(AS) $(AFLAGS) $(patsubst $(arch_kernel_build_dir)/%.o, $(arch_kernel_source_dir)/%.asm, $@) -o $@

build-x86_64: $(boot_object_files) $(arch_kernel_object_files) $(kernel_object_files)
	mkdir -p dist/x86_64 && \
	$(LD) $(LFLAGS) -o dist/x86_64/kernel.bin -T targets/x86_64/linker.ld $(boot_object_files) $(arch_kernel_object_files) $(kernel_object_files) $(x86_64_object_files) $(RUST_RLIBS) && \
	cp dist/x86_64/kernel.bin targets/x86_64/iso/boot/kernel.bin && \
	grub-mkrescue /usr/lib/grub/i386-pc -o dist/x86_64/kernel.iso targets/x86_64/iso

.PHONY: hdd-image

hdd-image: build-x86_64
	mkdir -p dist/x86_64 && \
	tmpdir=$$(mktemp -d); \
	core_img=$$tmpdir/core.img; \
	grub_cfg=$$tmpdir/grub.cfg; \
	printf 'set timeout=0\nset default=0\n\nmenuentry "Ares" {\n    multiboot (memdisk)/boot/kernel.bin\n    boot\n}\n' > $$grub_cfg && \
	grub-mkstandalone -O i386-pc \
		-o $$core_img \
		--modules="biosdisk part_msdos multiboot normal" \
		"boot/grub/grub.cfg=$$grub_cfg" \
		"boot/kernel.bin=dist/x86_64/kernel.bin" && \
	rm -f dist/x86_64/disk.img && \
	truncate -s 64M dist/x86_64/disk.img && \
	dd if=/usr/lib/grub/i386-pc/boot.img of=dist/x86_64/disk.img conv=notrunc status=none && \
	dd if=$$core_img of=dist/x86_64/disk.img bs=512 seek=1 conv=notrunc status=none && \
	rm -rf $$tmpdir

clean:
	rm -Rf ./dist
	rm -Rf ./build

run: build-x86_64
	qemu-system-x86_64 -cdrom dist/x86_64/kernel.iso \
										 -serial mon:stdio \
										 -serial file:kernel.log \
										 -d int,cpu_reset \
										 -no-reboot
