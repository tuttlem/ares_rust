# Agent Workflow

## Build
- From the repository root run `make clean && make build-x86_64` to produce the x86_64 kernel image and ISO under `dist/x86_64/`.

## Run (Headless)
- Boot the image without VGA by using QEMU in serial/headless mode:
  `timeout 120 qemu-system-x86_64 -drive file="dist/x86_64/hda.img",format=raw,if=ide,index=0,media=disk -cdrom dist/x86_64/kernel.iso -nographic -serial stdio`

## Timeouts & Shutdown
- Always wrap QEMU launches with a timeout (120s is usually sufficient) so automated runs do not hang.
- If QEMU outlives the timeout or becomes unresponsive, terminate it manually with `pkill qemu-system-x86_64`.
- After QEMU exits, confirm no lingering `qemu-system-x86_64` processes remain.
