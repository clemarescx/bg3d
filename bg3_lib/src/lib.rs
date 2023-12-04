#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
pub mod abstract_file_info;
mod bin_utils;
mod file_entry;
pub mod lsf_reader;
mod lspk_header;
pub mod package;
mod package_metadata;
pub mod package_reader;
pub mod package_version;

// hexadecimal values for "LSPK" signature
const LSPK_SIGNATURE: [u8; 4] = [0x4C, 0x53, 0x50, 0x4B];
