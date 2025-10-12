#!/bin/bash

qemu-system-x86_64 \
  -cdrom dist/x86_64/kernel.iso \
  -serial stdio \
  -serial file:kernel.log \
  -no-reboot
