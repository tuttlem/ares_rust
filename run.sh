#!/bin/bash
set -euo pipefail

ISO_PATH="dist/x86_64/kernel.iso"
DISK_PATH="dist/x86_64/disk.img"
HDD_PATH="dist/x86_64/hda.img"
LOG_PATH="kernel.log"

MODE="iso"
if [[ ${1:-} == "--disk" ]]; then
  MODE="disk"
  shift
fi

ensure_hdd() {
  if [[ ! -f "$HDD_PATH" ]]; then
    mkdir -p "$(dirname "$HDD_PATH")"
    truncate -s 64M "$HDD_PATH"
    echo "[run.sh] created blank disk at $HDD_PATH" >&2
  fi
}

if [[ "$MODE" == "iso" ]]; then
  if [[ ! -f "$ISO_PATH" ]]; then
    echo "error: $ISO_PATH not found. Build with ./domake build-x86_64." >&2
    exit 1
  fi
  ensure_hdd
  qemu-system-x86_64 \
    -cdrom "$ISO_PATH" \
    -drive file="$HDD_PATH",format=raw,if=ide,index=0,media=disk \
    -serial stdio \
    -serial file:$LOG_PATH \
    -no-reboot \
    "$@"
else
  if [[ ! -f "$DISK_PATH" ]]; then
    echo "error: $DISK_PATH not found. Build with ./domake hdd-image." >&2
    exit 1
  fi
  ensure_hdd
  qemu-system-x86_64 \
    -drive file="$DISK_PATH",format=raw,if=ide \
    -drive file="$HDD_PATH",format=raw,if=ide,index=1,media=disk \
    -serial stdio \
    -serial file:$LOG_PATH \
    -display none \
    -no-reboot \
    "$@"
fi
