use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;
use std::io::{prelude::*, Cursor, SeekFrom};

use serde::{de::DeserializeOwned, Deserialize};

use crate::abstract_file_info::CompressionMethod;
use crate::bin_utils::{self, ReadExt};
use crate::{abstract_file_info::PackagedFileInfo, package_reader::PackageReader};

#[derive(Debug, Default)]
pub struct LSFReader {
    pub version: Option<LSFVersion>,
    pub game_version: PackedVersion,
    pub metadata: LSFMetadataV6,
    pub names: Vec<Vec<String>>,
    pub node_infos: Vec<LSFNodeInfo>,
    pub attributes: Vec<LSFAttributeInfo>,
    pub values: Vec<u8>,
}

impl LSFReader {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn read(
        &mut self,
        package_reader: &mut PackageReader,
        pfi: &PackagedFileInfo,
    ) -> Result<Resource, String> {
        println!("Reading LSF file {}", pfi.name.to_string_lossy());
        let file_bytes = package_reader.decompress_file(pfi)?;
        let mut lsf_reader = Cursor::new(&file_bytes[..]);

        self.read_headers(&mut lsf_reader)?;

        self.names = {
            let names_bytes = self.decompress(
                &mut lsf_reader,
                self.metadata.strings_size_on_disk as usize,
                self.metadata.strings_uncompressed_size as usize,
                false,
            )?;
            let mut names_stream = Cursor::new(&names_bytes[..]);
            self.read_names(&mut names_stream)?
        };

        self.node_infos = {
            let nodes_bytes = self.decompress(
                &mut lsf_reader,
                self.metadata.nodes_size_on_disk as usize,
                self.metadata.nodes_uncompressed_size as usize,
                true,
            )?;

            let mut nodes_stream = Cursor::new(&nodes_bytes[..]);

            let long_nodes = self
                .version
                .as_ref()
                .is_some_and(|v| *v >= LSFVersion::VerExtendedNodes)
                && self.metadata.has_sibling_data == 1;

            if long_nodes {
                println!("v3 nodes");
                self.read_nodes::<LSFNodeEntryV3>(&mut nodes_stream)?
            } else {
                println!("v2 nodes");
                self.read_nodes::<LSFNodeEntryV2>(&mut nodes_stream)?
            }
        };

        self.attributes = {
            let attributes_bytes = self.decompress(
                &mut lsf_reader,
                self.metadata.attributes_size_on_disk as usize,
                self.metadata.attributes_uncompressed_size as usize,
                true,
            )?;

            let mut attributes_stream = Cursor::new(&attributes_bytes[..]);
            let has_sibling_data = self
                .version
                .as_ref()
                .is_some_and(|v| *v >= LSFVersion::VerExtendedNodes)
                && self.metadata.has_sibling_data == 1;

            if has_sibling_data {
                println!("v3 attributes");
                self.read_attributes_v3(&mut attributes_stream)?
            } else {
                println!("v2 attributes");
                self.read_attributes_v2(&mut attributes_stream)?
            }
        };

        self.values = self.decompress(
            &mut lsf_reader,
            self.metadata.values_size_on_disk as usize,
            self.metadata.values_uncompressed_size as usize,
            true,
        )?;

        let mut values_stream = Cursor::new(&self.values[..]);
        let mut resource = self.read_regions(&mut values_stream)?;
        resource.metadata.major_version = self.game_version.major;
        resource.metadata.minor_version = self.game_version.minor;
        resource.metadata.revision = self.game_version.revision;
        resource.metadata.build_number = self.game_version.build;

        Ok(resource)
    }

    fn read_regions(&self, stream: &mut Cursor<&[u8]>) -> Result<Resource, String> {
        let mut node_instances: Vec<Node> = Vec::with_capacity(self.node_infos.len());
        let mut regions: BTreeMap<String, usize> = BTreeMap::new();

        for node_info in self.node_infos.iter() {
            let node_data = self.read_node(node_info, stream)?;
            let node_name = node_data.name;

            if let Some(parent_index) = node_info.parent_index {
                let parent_idx = parent_index;
                let parent = if node_instances.get(parent_idx).is_none() {
                    None
                } else {
                    Some(parent_idx)
                };

                let node = Node {
                    kind: NodeKind::Node,
                    attributes: node_data.attributes.unwrap_or_default(),
                    name: node_name.clone(),
                    parent,
                    children: Default::default(),
                };

                let node_idx = node_instances.len();
                node_instances.push(node);
                node_instances
                    .get_mut(parent_idx)
                    .ok_or_else(|| {
                        format!(
                            "could not find parent node at index {parent_index} in node_instances"
                        )
                    })?
                    .append_child(&node_name, node_idx);
            } else {
                let kind = NodeKind::Region {
                    name: node_name.clone(),
                };
                let region = Node {
                    kind,
                    attributes: node_data.attributes.unwrap_or_default(),
                    name: node_name.clone(),
                    parent: None,
                    children: Default::default(),
                };

                let node_idx = node_instances.len();
                node_instances.push(region);
                regions.insert(node_name, node_idx);
            }
        }

        let mut resource = Resource::new();

        resource.regions = regions;
        resource.node_instances = node_instances;

        Ok(resource)
    }

    fn read_node(
        &self,
        defn: &LSFNodeInfo,
        stream: &mut Cursor<&[u8]>,
    ) -> Result<NodeData, String> {
        let name = self
            .names
            .get(defn.name_index as usize)
            .ok_or_else(|| {
                format!(
                    "failed getting node name collection at name_index {}",
                    defn.name_index
                )
            })?
            .get(defn.name_offset as usize)
            .ok_or_else(|| {
                format!(
                    "failed getting node name at name_offset {}",
                    defn.name_offset
                )
            })?
            .clone();

        let first_attribute_index = if let Some(idx) = defn.first_attribute_index {
            idx
        } else {
            return Ok(NodeData {
                name,
                attributes: None,
            });
        };

        let mut attribute = self.attributes.get(first_attribute_index).ok_or_else(|| {
            format!(
                "failed getting LSFAttributeInfo at first_attribute_index {first_attribute_index}"
            )
        })?;

        let mut attributes = HashMap::with_capacity(10);

        loop {
            stream
                .seek(SeekFrom::Start(attribute.data_offset as u64))
                .map_err(|e| {
                    format!(
                        "failed seeking attribute data in values at data_offset {}: {e}",
                        attribute.data_offset,
                    )
                })?;

            let type_id_enum: DataType = attribute.type_id.into();
            let value = self.read_attribute(type_id_enum, stream, attribute.length)?;

            let attr_name = self
                .names
                .get(attribute.name_index as usize)
                .ok_or_else(|| {
                    format!(
                        "failed getting attribute name collection at name_index {}",
                        attribute.name_index
                    )
                })?
                .get(attribute.name_offset as usize)
                .ok_or_else(|| {
                    format!(
                        "failed getting attribute name at name_offset {}",
                        attribute.name_offset
                    )
                })?
                .clone();

            attributes.insert(attr_name, value);

            if let Some(next_attribute_idx) = attribute.next_attribute_index {
                attribute = self.attributes.get(next_attribute_idx).ok_or_else(|| {
                    format!(
                    "failed getting LSFAttributeInfo at next_attribute_idx {next_attribute_idx}"
                )
                })?;
            } else {
                break;
            }
        }

        Ok(NodeData {
            name,
            attributes: Some(attributes),
        })
    }

    fn decompress(
        &self,
        stream: &mut Cursor<&[u8]>,
        size_on_disk: usize,
        uncompressed_size: usize,
        allow_chunked: bool,
    ) -> Result<Vec<u8>, String> {
        if size_on_disk == 0 && uncompressed_size != 0 {
            let mut uncompressed = vec![0; uncompressed_size];
            stream.read_exact(&mut uncompressed).map_err(|e| {
                format!("could not read {uncompressed_size} bytes from LSF file: {e}")
            })?;
            return Ok(uncompressed);
        }

        if size_on_disk == 0 && uncompressed_size == 0 {
            return Ok(vec![]);
        }

        let chunked = allow_chunked
            && self
                .version
                .as_ref()
                .is_some_and(|v| *v >= LSFVersion::VerChunkedCompress);
        let is_compressed = CompressionMethod::get(self.metadata.compression_flags)
            .is_some_and(|c| c != CompressionMethod::None);
        let compressed_size = if is_compressed {
            size_on_disk
        } else {
            uncompressed_size
        };

        let mut compressed = vec![0; compressed_size];
        stream
            .read_exact(&mut compressed)
            .map_err(|e| format!("could not read {compressed_size} bytes from LSF file: {e}"))?;
        let uncompressed = bin_utils::decompress(
            &compressed,
            uncompressed_size,
            self.metadata.compression_flags,
            chunked,
        )
        .map_err(|e| format!("failed to decompress LSF stream: {e}"))?;

        Ok(uncompressed)
    }

    fn read_headers(&mut self, mut stream: &mut Cursor<&[u8]>) -> Result<(), String> {
        let magic: LSFMagic = bincode::deserialize_from(&mut stream)
            .map_err(|e| format!("could not deserialize LSF magic number: {e}"))?;
        if magic.magic != LSFMagic::LSOF_SIGNATURE {
            let error_txt = format!(
                "invalid LSF signature; expected {:#x}, got {:#x}",
                LSFMagic::signature_u32(),
                u32::from_ne_bytes(magic.magic)
            );
            return Err(error_txt);
        }

        self.version = LSFVersion::get(magic.version as u64);
        if self.version.is_none() {
            let error_txt = format!("LSF version {} is not supported", magic.version);
            return Err(error_txt);
        }

        self.game_version = if magic.version >= LSFVersion::VerBG3ExtendedHeader as u32 {
            let engine_version = stream
                .read_i64()
                .map_err(|e| format!("failed to read engine_version (i64): {e}"))?;
            let game_version: PackedVersion = engine_version.into();
            // Workaround for merged LSF files with missing engine version number
            if game_version.major == 0 {
                PackedVersion {
                    major: 4,
                    minor: 0,
                    revision: 9,
                    build: 0,
                }
            } else {
                game_version
            }
        } else {
            let engine_version = stream
                .read_i32()
                .map_err(|e| format!("failed to read engine_version (pre-V5):{e}"))?;

            engine_version.into()
        };

        self.metadata = if magic.version < LSFVersion::VerBG3AdditionalBlob as u32 {
            let meta: LSFMetadataV5 = bincode::deserialize_from(stream)
                .map_err(|e| format!("failed to read LSFMetadata V5: {e}"))?;
            LSFMetadataV6::from(&meta)
        } else {
            bincode::deserialize_from(stream)
                .map_err(|e| format!("failed to read LSFMetadata V6: {e}"))?
        };
        Ok(())
    }

    fn read_names(&self, stream: &mut Cursor<&[u8]>) -> Result<Vec<Vec<String>>, String> {
        let mut num_hash_entries = stream
            .read_u32()
            .map_err(|e| format!("failed reading number of hash entries: {e}"))?;

        let mut names = Vec::with_capacity(num_hash_entries as usize);
        while num_hash_entries > 0 {
            num_hash_entries -= 1;

            let mut num_strings = stream
                .read_u16()
                .map_err(|e| format!("failed reading number of strings: {e}"))?;

            let mut hash = Vec::with_capacity(num_strings as usize);

            while num_strings > 0 {
                num_strings -= 1;
                let name_len = stream
                    .read_u16()
                    .map_err(|e| format!("failed reading name length: {e}"))?;

                let mut name_bytes = vec![0u8; name_len as usize];
                stream
                    .read_exact(&mut name_bytes)
                    .map_err(|e| format!("failed to read {name_len}-bytes long name: {e}"))?;
                let name = String::from_utf8_lossy(&name_bytes);
                hash.push(name.to_string());
            }

            names.push(hash);
        }

        Ok(names)
    }

    fn read_nodes<T>(&self, mut stream: &mut Cursor<&[u8]>) -> Result<Vec<LSFNodeInfo>, String>
    where
        T: DeserializeOwned + Into<LSFNodeInfo>,
    {
        let stream_len = stream
            .seek(SeekFrom::End(0))
            .map_err(|e| format!("failed to seek last byte in node stream: {e}"))?;

        stream
            .rewind()
            .map_err(|e| format!("failed to rewind node stream: {e}"))?;

        let struct_size = std::mem::size_of::<T>();
        let deserialize_count = stream_len as usize / struct_size;

        let mut node_infos = Vec::with_capacity(deserialize_count);

        while stream.position() < stream_len {
            let item: T = bincode::deserialize_from(&mut stream)
                .map_err(|e| format!("failed to read LSFNodeEntry bytes: {e}"))?;
            let resolved = item.into();
            node_infos.push(resolved);
        }

        Ok(node_infos)
    }

    fn read_attributes_v3(
        &self,
        mut stream: &mut Cursor<&[u8]>,
    ) -> Result<Vec<LSFAttributeInfo>, String> {
        let stream_len = stream
            .seek(SeekFrom::End(0))
            .map_err(|e| format!("failed to seek last byte in attribute v3 stream: {e}"))?;

        stream
            .rewind()
            .map_err(|e| format!("failed to rewind attribute v3 stream: {e}"))?;

        let mut attributes = vec![];
        while stream.position() < stream_len {
            let item: LSFAttributeEntryV3 = bincode::deserialize_from(&mut stream)
                .map_err(|e| format!("failed to read LSFAttributeEntryV3 bytes: {e}"))?;
            attributes.push(item.into());
        }

        Ok(attributes)
    }

    fn read_attributes_v2(
        &self,
        mut stream: &mut Cursor<&[u8]>,
    ) -> Result<Vec<LSFAttributeInfo>, String> {
        let stream_len = stream
            .seek(SeekFrom::End(0))
            .map_err(|e| format!("failed to seek last byte in attribute v2 stream: {e}"))?;

        stream
            .rewind()
            .map_err(|e| format!("failed to rewind attribute v2 stream: {e}"))?;

        let mut prev_attribute_refs: Vec<Option<usize>> = vec![];
        let mut data_offset = 0;
        let mut index = 0;

        let mut attributes: Vec<LSFAttributeInfo> = vec![];

        while stream.position() < stream_len {
            let attribute: LSFAttributeEntryV2 = bincode::deserialize_from(&mut stream)
                .map_err(|e| format!("failed to read LSFAttributeEntryV2 bytes: {e}"))?;

            let resolved = LSFAttributeInfo {
                name_index: (attribute.name_hash_table_index >> 16) as i32,
                name_offset: (attribute.name_hash_table_index & 0xffff) as i32,
                type_id: attribute.type_and_length & 0x3f,
                length: attribute.type_and_length >> 6,
                data_offset,
                next_attribute_index: None,
            };

            let node_index = attribute.node_index + 1;
            if prev_attribute_refs.len() > node_index as usize {
                if let Some(prev_ref) = prev_attribute_refs.get_mut(node_index as usize) {
                    if let Some(prev_ref) = prev_ref {
                        if let Some(prev_att) = attributes.get_mut(*prev_ref) {
                            prev_att.next_attribute_index = Some(index);
                        }
                    }
                    *prev_ref = Some(index);
                }
            } else {
                let padding_len = node_index as usize - prev_attribute_refs.len();
                prev_attribute_refs.extend(std::iter::repeat(None).take(padding_len));
                prev_attribute_refs.push(Some(index));
            }

            data_offset += resolved.length;
            attributes.push(resolved);
            index += 1;
        }

        Ok(attributes)
    }

    fn read_attribute(
        &self,
        type_id: DataType,
        stream: &mut Cursor<&[u8]>,
        length: u32,
    ) -> Result<NodeAttribute, String> {
        let attr = match type_id {
            DataType::String
            | DataType::Path
            | DataType::FixedString
            | DataType::LSString
            | DataType::WString
            | DataType::LSWString => {
                let value: String = read_string(stream, length)?;
                NodeAttribute {
                    ty: type_id,
                    value: NodeAttributeValue::String(value),
                }
            }

            DataType::TranslatedString => {
                let version;
                let mut value = None;

                if self
                    .version
                    .as_ref()
                    .is_some_and(|v| *v >= LSFVersion::VerBG3)
                    || self.game_version.major > 4
                    || (self.game_version.major == 4 && self.game_version.revision > 0)
                    || (self.game_version.major == 4
                        && self.game_version.revision == 0
                        && self.game_version.build >= 0x1A)
                {
                    version = stream.read_u16()?;
                } else {
                    version = 0;
                    let value_length = stream.read_i32()?;
                    value = Some(read_string(stream, value_length as u32)?);
                }

                let handle_length = stream.read_i32()?;
                let handle = read_string(stream, handle_length as u32)?;
                let str_value = TranslatedString {
                    version,
                    value,
                    handle,
                };

                NodeAttribute {
                    ty: type_id,
                    value: NodeAttributeValue::TranslatedString(str_value),
                }
            }

            DataType::TranslatedFSString => {
                let value = read_translated_fs_string(stream, self.version)?;
                NodeAttribute {
                    ty: type_id,
                    value: NodeAttributeValue::TranslatedFSString(value),
                }
            }

            DataType::ScratchBuffer => {
                let mut buf = vec![0; length as usize];
                stream.read_exact(&mut buf).map_err(|e| {
                    format!("failed to read ScratchBuffer attribute value (length: {length}): {e}")
                })?;

                NodeAttribute {
                    ty: type_id,
                    value: NodeAttributeValue::Bytes(buf),
                }
            }

            _ => read_attribute(stream, type_id)?,
        };

        Ok(attr)
    }
}

fn read_attribute(stream: &mut Cursor<&[u8]>, type_id: DataType) -> Result<NodeAttribute, String> {
    let attr = match type_id {
        DataType::None => NodeAttributeValue::None,
        DataType::Byte => {
            let value = stream.read_u8()?;
            NodeAttributeValue::Byte(value)
        }
        DataType::Short => {
            let value = stream.read_i16()?;
            NodeAttributeValue::Short(value)
        }
        DataType::UShort => {
            let value = stream.read_u16()?;
            NodeAttributeValue::UShort(value)
        }
        DataType::Int => {
            let value = stream.read_i32()?;
            NodeAttributeValue::Int(value)
        }
        DataType::UInt => {
            let value = stream.read_u32()?;
            NodeAttributeValue::UInt(value)
        }
        DataType::Float => {
            let value = stream.read_f32()?;
            NodeAttributeValue::Float(value)
        }
        DataType::Double => {
            let value = stream.read_f64()?;
            NodeAttributeValue::Double(value)
        }
        DataType::IVec2 => {
            let value = stream.read_i32_vec::<2>()?;
            NodeAttributeValue::IVec2(value)
        }
        DataType::IVec3 => {
            let value = stream.read_i32_vec::<3>()?;
            NodeAttributeValue::IVec3(value)
        }
        DataType::IVec4 => {
            let value = stream.read_i32_vec::<4>()?;
            NodeAttributeValue::IVec4(value)
        }
        DataType::Vec2 => {
            let value = stream.read_f32_vec::<2>()?;
            NodeAttributeValue::Vec2(value)
        }
        DataType::Vec3 => {
            let value = stream.read_f32_vec::<3>()?;
            NodeAttributeValue::Vec3(value)
        }
        DataType::Vec4 => {
            let value = stream.read_f32_vec::<4>()?;
            NodeAttributeValue::Vec4(value)
        }
        DataType::Mat2 => {
            let value = stream.read_f32_mat::<2, 2>()?;
            NodeAttributeValue::Mat2(value)
        }
        DataType::Mat3 => {
            let value = stream.read_f32_mat::<3, 3>()?;
            NodeAttributeValue::Mat3(value)
        }
        DataType::Mat3x4 => {
            let value = stream.read_f32_mat::<4, 3>()?;
            NodeAttributeValue::Mat3x4(value)
        }
        DataType::Mat4x3 => {
            let value = stream.read_f32_mat::<3, 4>()?;
            NodeAttributeValue::Mat4x3(value)
        }
        DataType::Mat4 => {
            let value = stream.read_f32_mat::<4, 4>()?;
            NodeAttributeValue::Mat4(value)
        }
        DataType::Bool => {
            let value = stream.read_u8()? != 0;
            NodeAttributeValue::Bool(value)
        }
        DataType::ULongLong => {
            let value = stream.read_u64()?;
            NodeAttributeValue::UInt64(value)
        }
        DataType::Long | DataType::Int64 => {
            let value = stream.read_i64()?;
            NodeAttributeValue::Int64(value)
        }
        DataType::Int8 => {
            let value = stream.read_i8()?;
            NodeAttributeValue::I8(value)
        }
        DataType::Uuid => {
            let value = stream.read_uuid()?;
            NodeAttributeValue::Uuid(value)
        }

        _ => {
            return Err(format!(
                "read_attribute not inplemented for type id {type_id:?}"
            ))
        }
    };

    Ok(NodeAttribute {
        ty: type_id,
        value: attr,
    })
}

fn read_string(stream: &mut Cursor<&[u8]>, length: u32) -> Result<String, String> {
    let mut bytes = vec![0; length as usize];
    stream
        .read_exact(&mut bytes)
        .map_err(|e| format!("could not read {length} bytes from attribute reader: {e}"))?;

    match bytes.last() {
        Some(0) => {
            let mut last_null = bytes.len() - 1;
            while last_null > 0 && bytes[last_null - 1] == 0 {
                last_null -= 1;
            }
            bytes.truncate(last_null);
            String::from_utf8(bytes)
                .map_err(|e| format!("error converting bytes to UTF8 string: {e}"))
        }
        Some(_) => Err(
            "error reading string from attribute reader: string is not null-terminated".to_string(),
        ),
        _ => Ok(String::new()),
    }
}
fn read_translated_fs_string(
    stream: &mut Cursor<&[u8]>,
    version: Option<LSFVersion>,
) -> Result<TranslatedFSString, String> {
    let mut str_version = 0;
    let mut value = None;
    if version.is_some_and(|v| v >= LSFVersion::VerBG3) {
        str_version = stream.read_u16()?;
    } else {
        let value_length = stream.read_i32()?;
        value = Some(read_string(stream, value_length as u32)?);
    }

    let handle_length = stream.read_i32()?;
    let handle = read_string(stream, handle_length as u32)?;

    let arguments_len = stream.read_i32()? as usize;
    let mut arguments = Vec::with_capacity(arguments_len);
    for _ in 0..arguments_len {
        let arg_key_length = stream.read_i32()?;
        let key = read_string(stream, arg_key_length as u32)?;

        let arg_string = read_translated_fs_string(stream, version)?;

        let arg_value_length = stream.read_i32()?;
        let value = read_string(stream, arg_value_length as u32)?;

        let arg = TranslatedFSStringArgument {
            key,
            string: arg_string,
            value,
        };
        arguments.push(arg);
    }

    let base = TranslatedString {
        version: str_version,
        value,
        handle,
    };

    Ok(TranslatedFSString { base, arguments })
}

#[derive(Default, Debug, PartialEq, Deserialize)]
pub enum NodeKind {
    #[default]
    Node,
    Region {
        name: String,
    },
}

#[derive(Default, Debug, PartialEq, Deserialize)]
pub struct Node {
    pub kind: NodeKind,
    pub name: String,
    pub parent: Option<usize>,
    pub attributes: HashMap<String, NodeAttribute>,
    pub children: BTreeMap<String, Vec<usize>>,
}

impl Node {
    fn append_child(&mut self, child_name: &str, child_idx: usize) {
        self.children
            .entry(child_name.to_string())
            .or_default()
            .push(child_idx);
    }
}

#[derive(Debug, PartialEq, Deserialize)]
pub enum DataType {
    None = 0,
    Byte = 1,
    Short = 2,
    UShort = 3,
    Int = 4,
    UInt = 5,
    Float = 6,
    Double = 7,
    IVec2 = 8,
    IVec3 = 9,
    IVec4 = 10,
    Vec2 = 11,
    Vec3 = 12,
    Vec4 = 13,
    Mat2 = 14,
    Mat3 = 15,
    Mat3x4 = 16,
    Mat4x3 = 17,
    Mat4 = 18,
    Bool = 19,
    String = 20,
    Path = 21,
    FixedString = 22,
    LSString = 23,
    ULongLong = 24,
    ScratchBuffer = 25,
    // Seems to be unused?
    Long = 26,
    Int8 = 27,
    TranslatedString = 28,
    WString = 29,
    LSWString = 30,
    Uuid = 31,
    Int64 = 32,
    TranslatedFSString = 33,
    Unknown,
}

impl DataType {
    pub fn max_i32() -> i32 {
        // Last supported datatype, always keep this one at the end
        // DT_Max = Self::DT_TranslatedFSString as isize,
        Self::max() as i32
    }

    pub fn max() -> Self {
        // Last supported datatype, always keep this one at the end
        // DT_Max = Self::DT_TranslatedFSString as isize,
        Self::TranslatedFSString
    }
}

impl TryFrom<DataType> for u32 {
    type Error = &'static str;
    fn try_from(value: DataType) -> Result<Self, Self::Error> {
        match value {
            DataType::Unknown => Err("No u32 value for DataType::UNKNOWN"),
            _ => Ok(value as u32),
        }
    }
}

impl From<u32> for DataType {
    fn from(val: u32) -> DataType {
        match val {
            0 => DataType::None,
            1 => DataType::Byte,
            2 => DataType::Short,
            3 => DataType::UShort,
            4 => DataType::Int,
            5 => DataType::UInt,
            6 => DataType::Float,
            7 => DataType::Double,
            8 => DataType::IVec2,
            9 => DataType::IVec3,
            10 => DataType::IVec4,
            11 => DataType::Vec2,
            12 => DataType::Vec3,
            13 => DataType::Vec4,
            14 => DataType::Mat2,
            15 => DataType::Mat3,
            16 => DataType::Mat3x4,
            17 => DataType::Mat4x3,
            18 => DataType::Mat4,
            19 => DataType::Bool,
            20 => DataType::String,
            21 => DataType::Path,
            22 => DataType::FixedString,
            23 => DataType::LSString,
            24 => DataType::ULongLong,
            25 => DataType::ScratchBuffer,
            26 => DataType::Long,
            27 => DataType::Int8,
            28 => DataType::TranslatedString,
            29 => DataType::WString,
            30 => DataType::LSWString,
            31 => DataType::Uuid,
            32 => DataType::Int64,
            33 => DataType::TranslatedFSString,
            _ => DataType::Unknown,
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct TranslatedString {
    version: u16,
    value: Option<String>,
    handle: String,
}

impl Display for TranslatedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(val) = self.value.as_ref() {
            f.write_str(val)
        } else {
            f.write_str("Option::None")
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct TranslatedFSString {
    base: TranslatedString,
    arguments: Vec<TranslatedFSStringArgument>,
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct TranslatedFSStringArgument {
    key: String,
    string: TranslatedFSString,
    value: String,
}

#[derive(Debug, PartialEq, Deserialize)]
pub enum NodeAttributeValue {
    None,
    String(String),
    TranslatedString(TranslatedString),
    TranslatedFSString(TranslatedFSString),
    Bytes(Vec<u8>),
    Byte(u8),
    Short(i16),
    UShort(u16),
    Int(i32),
    UInt(u32),
    Float(f32),
    Double(f64),
    IVec2([i32; 2]),
    IVec3([i32; 3]),
    IVec4([i32; 4]),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat2([[f32; 2]; 2]),
    Mat3([[f32; 3]; 3]),
    Mat3x4([[f32; 4]; 3]),
    Mat4x3([[f32; 3]; 4]),
    Mat4([[f32; 4]; 4]),
    Bool(bool),
    UInt64(u64),
    Int64(i64),
    I8(i8),
    Uuid(uuid::Uuid),
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct NodeAttribute {
    pub ty: DataType,
    pub value: NodeAttributeValue,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum LSFVersion {
    VerInitial = 0x01,
    VerChunkedCompress = 0x02,
    VerExtendedNodes = 0x03,
    VerBG3 = 0x04,
    VerBG3ExtendedHeader = 0x05,
    VerBG3AdditionalBlob = 0x06,
    VerBg3Patch3 = 0x07,
}

impl LSFVersion {
    fn get(n: u64) -> Option<Self> {
        let v = match n {
            0x01 => Self::VerInitial,
            0x02 => Self::VerChunkedCompress,
            0x03 => Self::VerExtendedNodes,
            0x04 => Self::VerBG3,
            0x05 => Self::VerBG3ExtendedHeader,
            0x06 => Self::VerBG3AdditionalBlob,
            0x07 => Self::VerBg3Patch3,
            _ => return None,
        };
        Some(v)
    }
}

#[derive(Debug, Deserialize, Default)]
struct LSFMetadataV5 {
    strings_uncompressed_size: u32,
    strings_size_on_disk: u32,
    nodes_uncompressed_size: u32,
    nodes_size_on_disk: u32,
    attributes_uncompressed_size: u32,
    attributes_size_on_disk: u32,
    values_uncompressed_size: u32,
    values_size_on_disk: u32,
    compression_flags: u8,
    #[allow(dead_code)]
    unknown_2: u8,
    #[allow(dead_code)]
    unknown_3: u16,
    has_sibling_data: u32,
}

#[derive(Debug, Deserialize, Default)]
pub struct LSFMetadataV6 {
    strings_uncompressed_size: u32,
    strings_size_on_disk: u32,
    #[allow(dead_code)]
    unknown: u64,
    nodes_uncompressed_size: u32,
    nodes_size_on_disk: u32,
    attributes_uncompressed_size: u32,
    attributes_size_on_disk: u32,
    values_uncompressed_size: u32,
    values_size_on_disk: u32,
    compression_flags: u8,
    #[allow(dead_code)]
    unknown_2: u8,
    #[allow(dead_code)]
    unknown_3: u16,
    has_sibling_data: u32,
}
impl From<&LSFMetadataV5> for LSFMetadataV6 {
    fn from(meta: &LSFMetadataV5) -> Self {
        Self {
            strings_uncompressed_size: meta.strings_uncompressed_size,
            strings_size_on_disk: meta.strings_size_on_disk,
            unknown: 0,
            nodes_uncompressed_size: meta.nodes_uncompressed_size,
            nodes_size_on_disk: meta.nodes_size_on_disk,
            attributes_uncompressed_size: meta.attributes_uncompressed_size,
            attributes_size_on_disk: meta.attributes_size_on_disk,
            values_uncompressed_size: meta.values_uncompressed_size,
            values_size_on_disk: meta.values_size_on_disk,
            compression_flags: meta.compression_flags,
            unknown_2: 0,
            unknown_3: 0,
            has_sibling_data: meta.has_sibling_data,
        }
    }
}

#[derive(Debug)]
pub struct PackedVersion {
    major: u32,
    minor: u32,
    revision: u32,
    build: u32,
}

impl Default for PackedVersion {
    fn default() -> Self {
        0.into()
    }
}

impl From<i64> for PackedVersion {
    fn from(packed: i64) -> Self {
        Self {
            major: ((packed >> 55) & 0x7f) as u32,
            minor: ((packed >> 47) & 0xff) as u32,
            revision: ((packed >> 31) & 0xffff) as u32,
            build: (packed & 0x7fffffff) as u32,
        }
    }
}
impl From<i32> for PackedVersion {
    fn from(packed: i32) -> Self {
        Self {
            major: ((packed >> 28) & 0x0f) as u32,
            minor: ((packed >> 24) & 0x0f) as u32,
            revision: ((packed >> 16) & 0xff) as u32,
            build: (packed & 0xffff) as u32,
        }
    }
}

#[derive(Deserialize)]
struct LSFMagic {
    magic: [u8; 4],
    version: u32,
}

impl LSFMagic {
    const LSOF_SIGNATURE: [u8; 4] = [0x4C, 0x53, 0x4F, 0x46];
    const fn signature_u32() -> u32 {
        u32::from_ne_bytes(Self::LSOF_SIGNATURE)
    }
}

#[derive(PartialEq, Deserialize)]
pub struct Resource {
    pub metadata: LSMetadata,
    pub regions: BTreeMap<String, usize>,
    pub node_instances: Vec<Node>,
}

impl Default for Resource {
    fn default() -> Self {
        Self {
            metadata: LSMetadata {
                major_version: 3,
                ..Default::default()
            },
            regions: Default::default(),
            node_instances: Default::default(),
        }
    }
}

impl Resource {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Default, Deserialize, PartialEq)]
pub struct LSMetadata {
    pub timestamp: u64,
    pub major_version: u32,
    pub minor_version: u32,
    pub revision: u32,
    pub build_number: u32,
}

impl LSMetadata {
    pub const CURRENT_MAJOR_VERSION: u32 = 33;
}

#[derive(Debug)]
pub struct LSFNodeInfo {
    pub parent_index: Option<usize>,
    pub name_index: i32,
    pub name_offset: i32,
    pub first_attribute_index: Option<usize>,
}

#[derive(Deserialize)]
pub struct LSFNodeEntryV3 {
    name_hash_table_index: u32,
    parent_index: i32,
    _next_sibling_index: i32,
    first_attribute_index: i32,
}
impl From<LSFNodeEntryV3> for LSFNodeInfo {
    fn from(val: LSFNodeEntryV3) -> Self {
        LSFNodeInfo {
            parent_index: if val.parent_index < 0 {
                None
            } else {
                Some(val.parent_index as usize)
            },
            name_index: (val.name_hash_table_index >> 16) as i32,
            name_offset: (val.name_hash_table_index & 0xffff) as i32,
            first_attribute_index: if val.first_attribute_index == -1 {
                None
            } else {
                Some(val.first_attribute_index as usize)
            },
        }
    }
}

#[derive(Deserialize)]
pub struct LSFNodeEntryV2 {
    name_hash_table_index: u32,
    first_attribute_index: i32,
    parent_index: i32,
}

impl From<LSFNodeEntryV2> for LSFNodeInfo {
    fn from(val: LSFNodeEntryV2) -> Self {
        LSFNodeInfo {
            parent_index: if val.parent_index < 0 {
                None
            } else {
                Some(val.parent_index as usize)
            },
            name_index: (val.name_hash_table_index >> 16) as i32,
            name_offset: (val.name_hash_table_index & 0xffff) as i32,
            first_attribute_index: if val.first_attribute_index == -1 {
                None
            } else {
                Some(val.first_attribute_index as usize)
            },
        }
    }
}

trait LSFNodeVEntry {
    fn name_index(&self) -> i32;
    fn name_offset(&self) -> i32;
}

#[derive(Debug)]
pub struct LSFAttributeInfo {
    pub name_index: i32,
    pub name_offset: i32,
    pub type_id: u32,
    pub length: u32,
    pub data_offset: u32,
    pub next_attribute_index: Option<usize>,
}

impl From<LSFAttributeEntryV3> for LSFAttributeInfo {
    fn from(value: LSFAttributeEntryV3) -> Self {
        Self {
            name_index: (value.name_hash_table_index >> 16) as i32,
            name_offset: (value.name_hash_table_index & 0xffff) as i32,
            type_id: value.type_and_length & 0x3f,
            length: value.type_and_length >> 6,
            data_offset: value.offset,
            next_attribute_index: (value.next_attribute_index >= 0)
                .then_some(value.next_attribute_index as usize),
        }
    }
}

#[derive(Deserialize)]
pub struct LSFAttributeEntryV3 {
    pub name_hash_table_index: u32,
    pub type_and_length: u32,
    pub next_attribute_index: i32,
    pub offset: u32,
}

#[derive(Deserialize)]
pub struct LSFAttributeEntryV2 {
    pub name_hash_table_index: u32,
    pub type_and_length: u32,
    pub node_index: i32,
}

#[derive(Debug)]
pub struct NodeData {
    name: String,
    attributes: Option<HashMap<String, NodeAttribute>>,
}
