#!/bin/bash
set -euo pipefail

ISO_PATH="dist/x86_64/kernel.iso"
DISK_PATH="dist/x86_64/disk.img"
LOG_PATH="kernel.log"

MODE="iso"
if [[ ${1:-} == "--disk" ]]; then
  MODE="disk"
  shift
fi

if [[ "$MODE" == "iso" ]]; then
  if [[ ! -f "$ISO_PATH" ]]; then
    echo "error: $ISO_PATH not found. Build with ./domake build-x86_64." >&2
    exit 1
  fi
  qemu-system-x86_64 \
    -cdrom "$ISO_PATH" \
    -serial stdio \
    -serial file:$LOG_PATH \
    -no-reboot \
    "$@"
else
  if [[ ! -f "$DISK_PATH" ]]; then
    echo "error: $DISK_PATH not found. Build with ./domake hdd-image." >&2
    exit 1
  fi
  qemu-system-x86_64 \
    -drive file="$DISK_PATH",format=raw,if=ide \
    -serial stdio \
    -serial file:$LOG_PATH \
    -display none \
    -no-reboot \
    "$@"
fi
