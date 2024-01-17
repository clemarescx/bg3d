use std::{fmt::Display, path::PathBuf};

#[derive(Debug)]
pub enum AbstractFileInfo {
    PackagedFileInfo {
        offset_in_file: u64,
        size_on_disk: usize,
        uncompressed_size: usize,
        archive_part: u8,
        flags: u8,
        crc: u32,
        name: PathBuf,
    },
    Unknown,
}

impl AbstractFileInfo {
    pub fn size(&self) -> usize {
        match self {
            AbstractFileInfo::PackagedFileInfo {
                flags,
                size_on_disk,
                uncompressed_size,
                ..
            } => {
                if (flags & 0x0f) == 0 {
                    *size_on_disk
                } else {
                    *uncompressed_size
                }
            }
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackagedFileInfo {
    pub offset_in_file: u64,
    pub size_on_disk: usize,
    pub uncompressed_size: usize,
    pub archive_part: u8,
    pub flags: u8,
    pub crc: u32,
    pub name: PathBuf,
}
impl PackagedFileInfo {
    pub fn size(&self) -> usize {
        if (self.flags & 0x0f) == 0 {
            self.size_on_disk
        } else {
            self.uncompressed_size
        }
    }
}

impl Display for PackagedFileInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "archive part: {archive_part}
CRC32: {crc}
flags: {flags:#b}
offset: {offset:#X}
size on disk: {size_on_disk}
uncompressed size: {uncompressed_size}",
            archive_part = self.archive_part,
            crc = self.crc,
            flags = self.flags,
            offset = self.offset_in_file,
            size_on_disk = formatted_size(self.size_on_disk),
            uncompressed_size = formatted_size(self.uncompressed_size)
        )
    }
}

fn formatted_size(s: usize) -> String {
    if s == 0 {
        return "0 B".to_string();
    }

    let size_delimiter = s.ilog10() / 3;

    let (div, unit) = match size_delimiter {
        0 => return format!("{s} B"),
        1 => (10u64.pow(3), "KB"),
        2 => (10u64.pow(6), "MB"),
        _ => (10u64.pow(9), "GB"),
    };
    let val = s as f64 / div as f64;
    format!("{val:.2} {unit} ({s} Bytes)")
}

#[derive(Debug, PartialEq)]
pub enum CompressionMethod {
    None = 0,
    Zlib = 1,
    LZ4 = 2,
    ZSTD = 3,
}

impl CompressionMethod {
    pub fn get(flags: u8) -> Option<Self> {
        let val = match flags & 0x0F {
            0 => Self::None,
            1 => Self::Zlib,
            2 => Self::LZ4,
            3 => Self::ZSTD,
            _ => return None,
        };

        Some(val)
    }
}
