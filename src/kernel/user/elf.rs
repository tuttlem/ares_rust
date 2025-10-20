use alloc::vec::Vec;

#[derive(Debug)]
pub enum ElfError {
    InvalidMagic,
    UnsupportedClass,
    UnsupportedEncoding,
    UnsupportedMachine,
    InvalidHeader,
    InvalidProgramHeader,
    NoLoadableSegments,
}

#[derive(Debug, Clone)]
pub struct ElfSegment {
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub offset: u64,
    pub flags: u32,
    pub align: u64,
}

#[derive(Debug, Clone)]
pub struct ElfImage {
    pub entry: u64,
    pub segments: Vec<ElfSegment>,
}

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ELF_MACHINE_X86_64: u16 = 0x3E;
const PT_LOAD: u32 = 1;

pub fn parse(bytes: &[u8]) -> Result<ElfImage, ElfError> {
    if bytes.len() < 64 {
        return Err(ElfError::InvalidHeader);
    }

    if bytes[0..4] != ELF_MAGIC {
        return Err(ElfError::InvalidMagic);
    }

    if bytes[4] != ELFCLASS64 {
        return Err(ElfError::UnsupportedClass);
    }

    if bytes[5] != ELFDATA2LSB {
        return Err(ElfError::UnsupportedEncoding);
    }

    let machine = read_u16(bytes, 18)?;
    if machine != ELF_MACHINE_X86_64 {
        return Err(ElfError::UnsupportedMachine);
    }

    let entry = read_u64(bytes, 24)?;
    let phoff = read_u64(bytes, 32)?;
    let phentsize = read_u16(bytes, 54)? as usize;
    let phnum = read_u16(bytes, 56)? as usize;

    if phentsize != 56 || phnum == 0 {
        return Err(ElfError::InvalidProgramHeader);
    }

    let mut segments = Vec::new();

    for index in 0..phnum {
        let offset = phoff as usize + index * phentsize;
        if offset + phentsize > bytes.len() {
            return Err(ElfError::InvalidProgramHeader);
        }

        let p_type = read_u32(bytes, offset)?;
        if p_type != PT_LOAD {
            continue;
        }

        let p_flags = read_u32(bytes, offset + 4)?;
        let p_offset = read_u64(bytes, offset + 8)?;
        let p_vaddr = read_u64(bytes, offset + 16)?;
        let p_filesz = read_u64(bytes, offset + 32)?;
        let p_memsz = read_u64(bytes, offset + 40)?;
        let p_align = read_u64(bytes, offset + 48)?;

        if p_memsz == 0 {
            continue;
        }

        segments.push(ElfSegment {
            vaddr: p_vaddr,
            filesz: p_filesz,
            memsz: p_memsz,
            offset: p_offset,
            flags: p_flags,
            align: p_align.max(1),
        });
    }

    if segments.is_empty() {
        return Err(ElfError::NoLoadableSegments);
    }

    Ok(ElfImage { entry, segments })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ElfError> {
    if offset + 2 > bytes.len() {
        return Err(ElfError::InvalidHeader);
    }
    Ok(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ElfError> {
    if offset + 4 > bytes.len() {
        return Err(ElfError::InvalidHeader);
    }
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ElfError> {
    if offset + 8 > bytes.len() {
        return Err(ElfError::InvalidHeader);
    }
    Ok(u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ]))
}

pub fn segment_flags_writable(flags: u32) -> bool {
    flags & 0x2 != 0
}

pub fn segment_flags_executable(flags: u32) -> bool {
    flags & 0x1 != 0
}
