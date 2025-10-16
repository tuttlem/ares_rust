use crate::klog;
use crate::process;
use crate::syscall;

const PATTERN_TAG: &[u8] = b"VFS-SMOKE";

pub fn scratch_smoke_test() -> bool {
    let Some(file) = crate::vfs::ata::AtaScratchFile::get() else {
        klog!("[vfs:test] scratch file unavailable\n");
        return false;
    };

    klog!("[vfs:test] scratch smoke begin\n");

    let Some(pid) = process::current_pid() else {
        klog!("[vfs:test] no current pid\n");
        return false;
    };

    let sector = file.bytes_per_sector();
    if sector > 512 {
        klog!("[vfs:test] unsupported sector size {}\n", sector);
        return false;
    }

    let mut write_buf = [0u8; 512];
    let mut read_buf = [0u8; 512];

    write_buf[..PATTERN_TAG.len().min(sector)].copy_from_slice(&PATTERN_TAG[..PATTERN_TAG.len().min(sector)]);
    for (index, byte) in write_buf[PATTERN_TAG.len().min(sector)..sector].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(17).wrapping_add(0x3Cu8);
    }

    match process::with_fd_mut(pid, process::SCRATCH_FD, |desc| desc.seek(0)) {
        Ok(Ok(())) => klog!("[vfs:test] seek before write ok\n"),
        Ok(Err(err)) => {
            klog!("[vfs:test] seek before write failed: {:?}\n", err);
            return false;
        }
        Err(err) => {
            klog!("[vfs:test] seek before write errored: {:?}\n", err);
            return false;
        }
    }

    let written = syscall::write(syscall::fd::SCRATCH, &write_buf[..sector]);
    if written != sector as u64 {
        klog!("[vfs:test] write syscall returned {} (expected {})\n", written, sector);
        return false;
    }

    klog!("[vfs:test] write syscall complete\n");

    match process::with_fd_mut(pid, process::SCRATCH_FD, |desc| desc.seek(0)) {
        Ok(Ok(())) => klog!("[vfs:test] seek before read ok\n"),
        Ok(Err(err)) => {
            klog!("[vfs:test] seek before read failed: {:?}\n", err);
            return false;
        }
        Err(err) => {
            klog!("[vfs:test] seek before read errored: {:?}\n", err);
            return false;
        }
    }

    let read = syscall::read(syscall::fd::SCRATCH, &mut read_buf[..sector]);
    if read != sector as u64 {
        klog!("[vfs:test] read syscall returned {} (expected {})\n", read, sector);
        return false;
    }

    klog!("[vfs:test] read syscall complete\n");

    match process::with_fd_mut(pid, process::SCRATCH_FD, |desc| desc.seek(0)) {
        Ok(_) => klog!("[vfs:test] final seek attempted\n"),
        Err(err) => {
            klog!("[vfs:test] final seek errored: {:?}\n", err);
        }
    }

    if write_buf[..sector] == read_buf[..sector] {
        klog!("[vfs:test] scratch read/write OK ({} bytes)\n", sector);
        true
    } else {
        for i in 0..sector {
            if write_buf[i] != read_buf[i] {
                klog!(
                    "[vfs:test] mismatch at byte {}: wrote 0x{:02X}, read 0x{:02X}\n",
                    i,
                    write_buf[i],
                    read_buf[i]
                );
                break;
            }
        }
        false
    }
}
