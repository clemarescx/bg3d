use flate2::bufread::ZlibDecoder;
use uuid::Uuid;

use crate::abstract_file_info::CompressionMethod;
use std::io::{prelude::*, Cursor};

pub fn decompress(
    compressed: &[u8],
    decompressed_size: usize,
    compression_flags: u8,
    chunked: bool,
) -> Result<Vec<u8>, String> {
    match CompressionMethod::get(compression_flags) {
        Some(val) => match val {
            CompressionMethod::LZ4 => {
                if chunked {
                    let br = Cursor::new(compressed);
                    let mut buf = vec![0; decompressed_size];
                    lz4_flex::frame::FrameDecoder::new(br)
                        .read_exact(&mut buf)
                        .map_err(|e| {
                            format!("failed to decompress LZ4 chunked (frame) file: {e}")
                        })?;
                    Ok(buf)
                } else {
                    lz4_flex::block::decompress(compressed, decompressed_size)
                        .map_err(|e| format!("failed to decompress LZ4 block file: {e}"))
                }
            }

            CompressionMethod::Zlib => {
                let mut br = ZlibDecoder::new(compressed);
                let mut buf = vec![0; decompressed_size];
                br.read_exact(&mut buf[..])
                    .map_err(|e| format!("failed to decompress zlib file: {e}"))?;

                Ok(buf)
            }
            CompressionMethod::None => Ok(compressed.to_vec()),
        },
        None => Err(format!(
            "unsupported compression method - flags {compression_flags}"
        )),
    }
}

pub trait ReadExt {
    fn read_u64(&mut self) -> Result<u64, String>;
    fn read_i64(&mut self) -> Result<i64, String>;
    fn read_u32(&mut self) -> Result<u32, String>;
    fn read_i32(&mut self) -> Result<i32, String>;
    fn read_u16(&mut self) -> Result<u16, String>;
    fn read_i16(&mut self) -> Result<i16, String>;
    fn read_u8(&mut self) -> Result<u8, String>;
    fn read_i8(&mut self) -> Result<i8, String>;
    fn read_f32(&mut self) -> Result<f32, String>;
    fn read_f64(&mut self) -> Result<f64, String>;
    fn read_i32_vec<const N: usize>(&mut self) -> Result<[i32; N], String>;
    fn read_f32_vec<const N: usize>(&mut self) -> Result<[f32; N], String>;
    fn read_f32_mat<const COLS: usize, const ROWS: usize>(
        &mut self,
    ) -> Result<[[f32; COLS]; ROWS], String>;
    fn read_uuid(&mut self) -> Result<Uuid, String>;
}

impl<T: Read> ReadExt for T {
    fn read_u64(&mut self) -> Result<u64, String> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading u64: {e}"))?;
        Ok(u64::from_le_bytes(buf))
    }

    fn read_i64(&mut self) -> Result<i64, String> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading i64: {e}"))?;
        Ok(i64::from_le_bytes(buf))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading u32: {e}"))?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_i32(&mut self) -> Result<i32, String> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading i32: {e}"))?;
        Ok(i32::from_le_bytes(buf))
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading u16: {e}"))?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading i16: {e}"))?;
        Ok(i16::from_le_bytes(buf))
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let mut buf = [0u8];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading u8: {e}"))?;
        Ok(u8::from_le_bytes(buf))
    }

    fn read_i8(&mut self) -> Result<i8, String> {
        let mut buf = [0u8];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading i8: {e}"))?;
        Ok(i8::from_le_bytes(buf))
    }

    fn read_f32(&mut self) -> Result<f32, String> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading f32: {e}"))?;
        Ok(f32::from_le_bytes(buf))
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        let mut buf = [0; 8];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading f64: {e}"))?;
        Ok(f64::from_le_bytes(buf))
    }

    fn read_i32_vec<const N: usize>(&mut self) -> Result<[i32; N], String> {
        let mut value = [0; N];
        for v in value.iter_mut() {
            *v = self.read_i32()?;
        }
        Ok(value)
    }

    fn read_f32_vec<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut value = [0f32; N];
        for v in value.iter_mut() {
            *v = self.read_f32()?;
        }
        Ok(value)
    }

    fn read_f32_mat<const COLS: usize, const ROWS: usize>(
        &mut self,
    ) -> Result<[[f32; COLS]; ROWS], String> {
        let mut mat = [[0f32; COLS]; ROWS];
        for row in mat.iter_mut() {
            for col in row.iter_mut() {
                *col = self.read_f32()?;
            }
        }
        Ok(mat)
    }

    fn read_uuid(&mut self) -> Result<Uuid, String> {
        let mut buf = [0u8; 16];
        self.read_exact(&mut buf)
            .map_err(|e| format!("failed reading uuid (16 bytes): {e}"))?;
        Ok(Uuid::from_bytes(buf))
    }
}
