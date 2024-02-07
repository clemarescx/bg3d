use std::fs::DirBuilder;
use std::io::{prelude::*, BufReader, BufWriter, Cursor, SeekFrom};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::abstract_file_info::{CompressionMethod, PackagedFileInfo};
use crate::bin_utils;
use crate::bin_utils::ReadExt;
use crate::file_entry::{FileEntry18, SIZE_OF_FILE_ENTRY_18};
use crate::lsf_reader::{LSFReader, Resource};
use crate::lspk_header::LSPKHeader16;
use crate::package_version::PackageVersion;
use crate::{package::Package, LSPK_SIGNATURE};

pub struct PackageReader {
    file_name: String,
    reader: Cursor<Vec<u8>>,
}

impl PackageReader {
    pub fn new(path: &Path) -> Result<Self, String> {
        let (path, file_name) = {
            let path = Path::new(&path);
            (
                path.to_path_buf(),
                path.file_name()
                    .ok_or("invalid file name")?
                    .to_string_lossy(),
            )
        };

        let file = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|e| format!("failed opening {}: {e}", path.to_string_lossy()))?;

        let mut buffer: Vec<u8> = vec![];
        let _ = BufReader::new(file)
            .read_to_end(&mut buffer)
            .map_err(|e| format!("could not read {} in memory: {e}", path.to_string_lossy()))?;

        let reader = Cursor::new(buffer);

        let package_reader = Self {
            file_name: file_name.to_string(),
            reader,
        };

        Ok(package_reader)
    }

    pub fn read(&mut self) -> Result<Package, String> {
        println!("Reading {} ...", self.file_name);
        let mut signature = [0; 4];

        self.reader
            .read_exact(&mut signature)
            .map_err(|e| format!("could not read 4-byte signature from beginning: {e}"))?;

        if signature != LSPK_SIGNATURE {
            return Err("not V10".to_string());
        }

        println!("found V10 package headers");

        let version = self
            .reader
            .read_u32()
            .map_err(|e| format!("could not read 4-byte version: {e}"))?;

        if version == PackageVersion::V18 as u32 {
            println!("found v18 package");
            self.reader
                .seek(SeekFrom::Current(-4))
                .map_err(|e| format!("failed to rewind 4 bytes: {e}"))?;
            self.read_package_v18(version)
        } else {
            Err("unknown BG3 save version format".to_string())
        }
    }

    fn read_package_v18(&mut self, version: u32) -> Result<Package, String> {
        let mut package = Package::new();
        let header: LSPKHeader16 = bincode::deserialize_from(&mut self.reader)
            .map_err(|e| format!("failed to deserialize LSPKHeader16: {e}"))?;

        if header.version != version {
            return Err("package version is not v18, deserialization messed up".to_string());
        }

        package.metadata.flags = header.flags;
        package.metadata.priority = header.priority;
        package.version = PackageVersion::V18;

        self.reader
            .seek(SeekFrom::Start(header.file_list_offset))
            .map_err(|e| format!("seek to file list offset failed: {e}"))?;

        package.files = self.read_file_list_v18()?;

        Ok(package)
    }

    fn read_file_list_v18(&mut self) -> Result<Vec<PackagedFileInfo>, String> {
        let num_files = self
            .reader
            .read_u32()
            .map_err(|e| format!("failed reading number of files bytes: {e}"))?;

        let compressed_size = self
            .reader
            .read_u32()
            .map_err(|e| format!("failed reading compressed size bytes: {e}"))?;

        let mut compressed_file_list = vec![0u8; compressed_size as usize];
        let read = self
            .reader
            .read(&mut compressed_file_list)
            .map_err(|e| format!("failed reading compressed file list bytes: {e}"))?;

        if read == 0 {
            return Err("0-sized compressed file list".to_string());
        }

        let filebuffer_size = SIZE_OF_FILE_ENTRY_18 * num_files as usize;
        let uncompressed_list = lz4_flex::decompress(&compressed_file_list, filebuffer_size)
            .map_err(|e| format!("failed to decompress LZ4 package: {e}"))?;

        if uncompressed_list.len() != filebuffer_size {
            return Err(format!("LZ4 compressor disagrees about the size of file headers; expected {filebuffer_size}, got {}", uncompressed_list.len()));
        }

        // The following doesn't work, for some reason:
        //      let uf_reader = BufReader::new(&uncompressed_list[..]);
        //      let entries: Vec<FileEntry18> = match bincode::deserialize_from(uf_reader).unwrap();
        // That's why we have to iterate over the bytes in chunks

        let file_entries = uncompressed_list
            .chunks_exact(SIZE_OF_FILE_ENTRY_18)
            .map(|c| {
                bincode::deserialize::<FileEntry18>(c)
                    .map_err(|e| format!("failed to deserialize FileEntry18 from binary: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let files = file_entries
            .into_iter()
            .map(|file_entry| {
                let name_len = file_entry
                    .name
                    .iter()
                    .copied()
                    .take_while(|c| *c != 0)
                    .count();
                let name = String::from_utf8_lossy(&file_entry.name[0..name_len]).to_string();
                let compression_method: u32 = file_entry.flags as u32 & 0x0F;
                if compression_method > 3 || (file_entry.flags as u32 & !0x7F) != 0 {
                    return Err(format!(
                        "File '{}' has unsupported flags: {}",
                        &name, file_entry.flags
                    ));
                }

                Ok(PackagedFileInfo {
                    offset_in_file: (file_entry.offset_in_file_1 as u64)
                        | ((file_entry.offset_in_file_2 as u64) << 32),
                    size_on_disk: file_entry.size_on_disk as usize,
                    uncompressed_size: file_entry.uncompressed_size as usize,
                    archive_part: file_entry.archive_part,
                    flags: file_entry.flags,
                    crc: 0,
                    name: PathBuf::from(name),
                })
            })
            .collect::<Result<_, String>>()?;

        Ok(files)
    }

    pub fn extract_all_files(
        &mut self,
        package: &Package,
        output_path: Option<PathBuf>,
    ) -> Result<(), String> {
        let files = &package.files;
        let total_size: usize = files.iter().map(|p| p.size()).sum();
        let mut current_size = 0;
        let root_output_dir = if let Some(o) = output_path {
            o
        } else {
            PathBuf::from("extracted")
        };
        for file in files {
            let file_size = file.size();
            current_size += file_size;

            let pfi @ PackagedFileInfo {
                name,
                flags,
                size_on_disk,
                ..
            } = file;
            println!(
                "unpacking {} ({} bytes) ({} out of {} bytes)",
                name.to_string_lossy(),
                file_size,
                current_size,
                total_size
            );
            let file_output_dir = if let Some(parent_dir) = name.parent() {
                root_output_dir.join(parent_dir)
            } else {
                root_output_dir.clone()
            };

            if !file_output_dir.exists() {
                if let Err(e) = DirBuilder::new().create(&file_output_dir) {
                    return Err(format!(
                        "failed to create directory '{}': {e}",
                        file_output_dir.to_string_lossy()
                    ));
                };
            }

            let file_path = if let Some(file_name) = pfi.name.file_name() {
                file_output_dir.join(file_name)
            } else {
                return Err("no file name".to_string());
            };

            if (flags & 0x0F) == CompressionMethod::None as u8 {
                todo!("implement uncompressed stream");
            }

            if *size_on_disk > 0x7fffffff {
                return Err(format!(
                    "File '{}' is over 2GB ({} bytes), which is not supported yet!",
                    &name.to_string_lossy(),
                    size_on_disk
                ));
            }

            let uncompressed = self.decompress_file(pfi)?;
            let out_file = File::options()
                .write(true)
                .truncate(true)
                .create(true)
                .open(&file_path)
                .map_err(|e| {
                    format!(
                        "failed to open/create file '{}' for write: {e}",
                        &file_path.to_string_lossy()
                    )
                })?;

            let mut bw = BufWriter::new(out_file);
            bw.write_all(&uncompressed)
                .map_err(|e| format!("failed to write all bytes to file: {e}"))?;
            bw.flush()
                .map_err(|e| format!("failed to flush the bufwriter: {e}"))?;
        }
        Ok(())
    }

    pub fn decompress_file(&mut self, pfi: &PackagedFileInfo) -> Result<Vec<u8>, String> {
        let mut compressed = vec![0u8; pfi.size_on_disk];

        self.reader
            .seek(SeekFrom::Start(pfi.offset_in_file))
            .map_err(|e| format!("could not seek to offset {}: {e}", pfi.offset_in_file))?;

        self.reader.read_exact(&mut compressed).map_err(|e| {
            format!(
                "failed to read {} bytes from archive: {e}",
                pfi.size_on_disk
            )
        })?;

        if pfi.crc != 0 {
            todo!("compute and check crc32");
        }

        bin_utils::decompress(&compressed, pfi.uncompressed_size, pfi.flags, false)
    }

    pub fn extract_file(
        &mut self,
        file: &PackagedFileInfo,
        output_path: Option<PathBuf>,
    ) -> Result<(), String> {
        let root_output_dir = if let Some(o) = output_path {
            o
        } else {
            PathBuf::from("extracted")
        };

        let pfi @ PackagedFileInfo {
            name,
            flags,
            size_on_disk,
            ..
        } = file;
        let file_output_dir = if let Some(parent_dir) = name.parent() {
            root_output_dir.join(parent_dir)
        } else {
            root_output_dir.clone()
        };

        if !file_output_dir.exists() {
            if let Err(e) = DirBuilder::new().create(&file_output_dir) {
                return Err(format!(
                    "failed to create directory '{}': {e}",
                    file_output_dir.to_string_lossy()
                ));
            };
        }

        let file_path = if let Some(file_name) = pfi.name.file_name() {
            file_output_dir.join(file_name)
        } else {
            return Err("no file name".to_string());
        };

        if (flags & 0x0F) == CompressionMethod::None as u8 {
            todo!("implement uncompressed stream");
        }

        if *size_on_disk > 0x7fffffff {
            return Err(format!(
                "File '{}' is over 2GB ({size_on_disk} bytes), which is not supported yet!",
                &name.to_string_lossy()
            ));
        }

        let uncompressed = self.decompress_file(pfi)?;
        let out_file = File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&file_path)
            .map_err(|e| {
                format!(
                    "failed to open/create file '{}' for write: {e}",
                    &file_path.to_string_lossy()
                )
            })?;

        let mut bw = BufWriter::new(out_file);
        bw.write_all(&uncompressed)
            .map_err(|e| format!("failed to write all bytes to file: {e}"))?;
        bw.flush()
            .map_err(|e| format!("failed to flush the bufwriter: {e}"))
    }

    pub fn load_globals(&mut self, package: &Package) -> Result<Resource, String> {
        let globals_info = package
            .files
            .iter()
            .find(|pfi| {
                pfi.name
                    .to_string_lossy()
                    .eq_ignore_ascii_case("globals.lsf")
            })
            .ok_or("could not find globals.lsf in packaged files")?;

        LSFReader::new().read(self, globals_info)
    }
}
