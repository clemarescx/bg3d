use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LSPKHeader16 {
    pub version: u32,
    pub file_list_offset: u64,
    pub _file_list_size: u32,
    pub flags: u8,
    pub priority: u8,
    pub _md5: [u8; 16],
    pub _num_parts: u16,
}
