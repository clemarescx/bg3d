use bincode::Decode;
use serde::Deserialize;
use serde_big_array::BigArray;

#[derive(Debug, Deserialize, Decode)]
pub struct FileEntry18 {
    #[serde(with = "BigArray")]
    pub name: [u8; 256],
    pub offset_in_file_1: u32,
    pub offset_in_file_2: u16,
    pub archive_part: u8,
    pub flags: u8,
    pub size_on_disk: u32,
    pub uncompressed_size: u32,
}

pub const SIZE_OF_FILE_ENTRY_18: usize = std::mem::size_of::<FileEntry18>();
