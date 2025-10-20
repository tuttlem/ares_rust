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
                 $(wildcard $(RUST_LIBDIR)/librustc_std_workspace_core-*.rlib) \
                 $(wildcard $(RUST_LIBDIR)/libpanic_abort-*.rlib)

boot_source_dir       := src/arch/x86_64/boot
boot_build_dir        := build/x86_64/boot
boot_source_files     := $(shell find $(boot_source_dir) -name "*.asm")
boot_asm_object_files := $(patsubst $(boot_source_dir)/%.asm, $(boot_build_dir)/%.o, $(boot_source_files))

boot_object_files     := $(boot_asm_object_files)

kernel_source_files   := src/kernel/kmain.rs
kernel_object_files   := build/kernel/kmain.o

OUTPUT_BIN ?= dist/x86_64/kernel.bin
OUTPUT_ISO ?= dist/x86_64/kernel.iso
KERNEL_CFG ?=
ISO_ROOT   ?= targets/x86_64/iso

arch_kernel_source_dir        := src/arch/x86_64/kernel
arch_kernel_build_dir         := build/arch/x86_64/kernel
arch_kernel_asm_source_files  := $(shell find $(arch_kernel_source_dir) -name "*.asm" 2>/dev/null)
arch_kernel_asm_object_files  := $(patsubst $(arch_kernel_source_dir)/%.asm, $(arch_kernel_build_dir)/%.o, $(arch_kernel_asm_source_files))

arch_kernel_object_files      := $(arch_kernel_asm_object_files)

.PHONY: build-x86_64 test-kernel test qemu-test test-iso-root

all: build-x86_64

USER_TARGET := user/hello
USER_BIN := target/$(RUST_TARGET)/release/hello

.PHONY: user-bins

user-bins:
	cargo build --release --target $(RUST_TARGET) --manifest-path $(USER_TARGET)/Cargo.toml
	mkdir -p $(ISO_ROOT)/bin
	cp $(USER_BIN) $(ISO_ROOT)/bin/hello

$(boot_asm_object_files): $(boot_build_dir)/%.o : $(boot_source_dir)/%.asm
	mkdir -p $(dir $@) && \
	$(AS) $(AFLAGS) $(patsubst $(boot_build_dir)/%.o, $(boot_source_dir)/%.asm, $@) -o $@

$(kernel_object_files): build/kernel/%.o : src/kernel/%.rs
	mkdir -p $(dir $@) && \
	$(RUSTC) $(RUSTFLAGS) $(KERNEL_CFG) --target $(RUST_TARGET) --emit=obj -o $@ --crate-type=lib $<

$(arch_kernel_asm_object_files): $(arch_kernel_build_dir)/%.o : $(arch_kernel_source_dir)/%.asm
	mkdir -p $(dir $@) && \
	$(AS) $(AFLAGS) $(patsubst $(arch_kernel_build_dir)/%.o, $(arch_kernel_source_dir)/%.asm, $@) -o $@

build-x86_64: user-bins $(boot_object_files) $(arch_kernel_object_files) $(kernel_object_files)
	mkdir -p dist/x86_64 && \
	$(LD) $(LFLAGS) -o $(OUTPUT_BIN) -T targets/x86_64/linker.ld $(boot_object_files) $(arch_kernel_object_files) $(kernel_object_files) $(x86_64_object_files) $(RUST_RLIBS) && \
	cp $(OUTPUT_BIN) $(ISO_ROOT)/boot/kernel.bin && \
	grub-mkrescue /usr/lib/grub/i386-pc -o $(OUTPUT_ISO) $(ISO_ROOT)

clean:
	rm -Rf ./distWhe
	rm -Rf ./build

run: build-x86_64
	qemu-system-x86_64 -cdrom dist/x86_64/kernel.iso \
										 -serial mon:stdio \
										 -serial file:kernel.log \
										 -d int,cpu_reset \
										 -no-reboot

TEST_ISO := dist/x86_64/kernel-test.iso

test-kernel: clean
test-kernel: KERNEL_CFG = --cfg kernel_test
test-kernel: OUTPUT_BIN = dist/x86_64/kernel-test.bin
test-kernel: OUTPUT_ISO = $(TEST_ISO)
test-kernel: ISO_ROOT = build/iso
test-kernel: test-iso-root
test-kernel: build-x86_64

test:
	cargo test -p ares-core

qemu-test: test-kernel
	qemu-system-x86_64 -cdrom $(TEST_ISO) \
					 -device isa-debug-exit,iobase=0xf4,iosize=0x01 \
					 -serial stdio \
					 -display none \
					 -no-reboot || test $$? -eq 1

test-iso-root:
	rm -rf build/iso
	mkdir -p build
	cp -r targets/x86_64/iso build/iso
	@if [ -n "$(FILTER)" ]; then \
		printf 'set timeout=0\nset default=0\n\nmenuentry "my os" {\n\tmultiboot2 /boot/kernel.bin test=%s\n\tboot\n}\n' "$(FILTER)" > build/iso/boot/grub/grub.cfg; \
	fi
