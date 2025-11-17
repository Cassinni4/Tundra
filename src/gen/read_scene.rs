use anyhow::anyhow;
use binrw::{BinRead, BinWrite, BinReaderExt, BinWriterExt, Endian, NullString};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use modular_bitfield::prelude::*;

// OCT Header structure
#[derive(BinRead, BinWrite, Debug)]
pub struct OctHeader {
    #[brw(pad_before = 4)]
    pub string_table_size: u32,
    pub data_tree_size: u32,
}

// Node structures for OCT files
#[derive(Debug)]
pub struct Node {
    pub id: String,
    pub data: NodeData,
}

#[derive(Debug)]
pub enum NodeData {
    Container(Vec<Node>),
    String(String),
    StringVec(Vec<String>),
    Float(f32),
    FloatVec(Vec<f32>),
    Int(i32),
    IntVec(Vec<i32>),
    Uuid(Uuid),
    Binary(Vec<u8>),
}

pub struct RawNode {
    pub level: u8,
    pub node: Node,
}

// Bitfield for node header
#[bitfield]
#[repr(u16)]
struct NodeHeader {
    r#type: Type,
    name: bool,
    data_type: DataType,
    len_size: B2,  // always +1
    int_size: B2,  // always +1
    level: B6,
}

#[derive(Debug, BitfieldSpecifier)]
#[bits = 2]
enum Type {
    None,
    Container,
    Vec,
    Scalar,
}

#[derive(Debug, BitfieldSpecifier)]
#[bits = 3]
enum DataType {
    None,
    String,
    Float,
    Int,
    Binary,
}

// Container data for serialization
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum ContainerData {
    Single(Data),
    Multiple(Vec<Data>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum Data {
    Container(IndexMap<String, ContainerData>),
    Binary(#[serde(with = "base64")] Vec<u8>),
    Uuid(Uuid),
    Int(i32),
    IntVec(Vec<i32>),
    Float(#[serde(deserialize_with = "deserialize_f64_null_as_nan")] f32),
    FloatVec(#[serde(deserialize_with = "deserialize_vec_f64_null_as_nan")] Vec<f32>),
    String(String),
    StringVec(Vec<String>),
}

mod base64 {
    use base64::{engine::general_purpose, Engine as _};
    use serde::{Deserialize, Serialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        let base64 = general_purpose::STANDARD_NO_PAD.encode(v);
        String::serialize(&format!("base64:{base64}"), s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let base64 = String::deserialize(d)?;
        match base64.strip_prefix("base64:") {
            None => Err(serde::de::Error::custom("missing \"base64:\" prefix")),
            Some(base64) => general_purpose::STANDARD_NO_PAD
                .decode(base64.as_bytes())
                .map_err(serde::de::Error::custom),
        }
    }
}

fn deserialize_f64_null_as_nan<'de, D: Deserializer<'de>>(des: D) -> Result<f32, D::Error> {
    let optional = Option::<f32>::deserialize(des)?;
    Ok(optional.unwrap_or(f32::NAN))
}

fn deserialize_vec_f64_null_as_nan<'de, D: Deserializer<'de>>(
    des: D,
) -> Result<Vec<f32>, D::Error> {
    Ok(
        Vec::<Option<f32>>::deserialize(des)?
            .iter()
            .map(|val| val.unwrap_or(f32::NAN))
            .collect(),
    )
}

// Animation data structures
#[derive(Debug, Clone)]
pub struct AnimationInfo {
    pub name: String,
    pub filename: String,
    pub metadata: Option<IndexMap<String, ContainerData>>,
}

#[derive(Debug, Clone)]
pub struct AnimationChannel {
    pub name: String,
    pub priority_order: Option<f32>,
    pub channel_index: Option<i32>,
    pub weight: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct AnimationData {
    pub version: String,
    pub model_filename: String,
    pub channels: Vec<AnimationChannel>,
    pub animations: Vec<AnimationInfo>,
}

// Main OCT file handler
pub struct SceneFileHandler {
    pub current_scene: Option<IndexMap<String, ContainerData>>,
    pub extracted_textures: Vec<TextureInfo>,
    pub endian: Option<Endian>,
    pub animation_data: Option<AnimationData>,
    pub current_bent_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TextureInfo {
    pub name: String,
    pub path: PathBuf,
    pub data: Vec<u8>,
}

// Game type enum for texture extraction
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameType {
    DisneyInfinity30,
    Cars2TheVideoGame,
    Cars2Arcade,
    Cars3DrivenToWinXB1,
    ToyShit3,
}

impl SceneFileHandler {
    pub fn new() -> Self {
        Self {
            current_scene: None,
            extracted_textures: Vec::new(),
            endian: None,
            animation_data: None,
            current_bent_path: None,
        }
    }

    pub fn load_scene_file<R: Read + Seek>(&mut self, reader: &mut R) -> anyhow::Result<()> {
        let mut magic: [u8; 8] = [0u8; 8];
        reader.read_exact(&mut magic)?;

        let endian = match magic {
            [0x29, 0x76, 0x01, 0x45, 0xcd, 0xcc, 0x8c, 0x3f] => Endian::Little,
            [0x45, 0x01, 0x76, 0x29, 0x3f, 0x8c, 0xcc, 0xcd] => Endian::Big,
            _ => return Err(anyhow!("Invalid magic: {magic:x?}")),
        };

        self.endian = Some(endian);
        let header: OctHeader = reader.read_type(endian)?;

        // 40 byte padding
        reader.seek(SeekFrom::Current(40))?;

        let start = reader.stream_position()?;
        let mut string_table = Vec::new();
        while (reader.stream_position()? - start) < header.string_table_size as u64 {
            let null_string: NullString = reader.read_type(endian)?;
            string_table.push(null_string.to_string());
        }

        let start = reader.stream_position()?;

        let RawNode { level, node } = reader.read_type_args(endian, string_table.as_slice())?;

        let root_level = level;
        let mut root_node = node;

        while (reader.stream_position()? - start) < header.data_tree_size as u64 {
            let RawNode { level, node } = reader.read_type_args(endian, string_table.as_slice())?;

            let mut curr_level = root_level;
            let mut curr_node = &mut root_node;

            while curr_level < level {
                curr_level += 1;
                let nodes = if let NodeData::Container(children) = &mut curr_node.data {
                    children
                } else {
                    return Err(anyhow!("Expected container"));
                };

                if curr_level == level {
                    nodes.push(node);
                    break;
                } else {
                    curr_node = nodes.last_mut().unwrap();
                }
            }
        }

        if let Data::Container(children) = root_node.data.try_into()? {
            self.current_scene = Some(children);
            Ok(())
        } else {
            Err(anyhow!("Expected root node to be a container"))
        }
    }

    pub fn load_bent_file<P: AsRef<Path>>(&mut self, path: P) -> anyhow::Result<()> {
        let mut file = fs::File::open(&path)?;
        self.load_bent_file_reader(&mut file)?;
        self.current_bent_path = Some(path.as_ref().to_path_buf());
        Ok(())
    }

    pub fn load_bent_file_reader<R: Read + Seek>(&mut self, reader: &mut R) -> anyhow::Result<()> {
        // Load the BENT file using the same OCT parsing logic
        self.load_scene_file(reader)?;
        
        // Parse the loaded scene data into animation data
        if let Some(scene_data) = &self.current_scene {
            self.animation_data = Some(self.parse_animation_data(scene_data)?);
        }
        
        Ok(())
    }

    fn parse_animation_data(&self, scene_data: &IndexMap<String, ContainerData>) -> anyhow::Result<AnimationData> {
        let mut version = String::new();
        let mut model_filename = String::new();
        let mut channels = Vec::new();
        let mut animations = Vec::new();

        // Parse version
        if let Some(ContainerData::Single(Data::String(ver))) = scene_data.get("Version") {
            version = ver.clone();
        }

        // Parse model information
        if let Some(ContainerData::Single(Data::Container(model))) = scene_data.get("Model") {
            if let Some(ContainerData::Single(Data::String(filename))) = model.get("Filename") {
                model_filename = filename.clone();
            }

            // Parse channels
            if let Some(ContainerData::Single(Data::Container(channels_data))) = model.get("Channels") {
                for (key, channel_data) in channels_data {
                    if key.starts_with("Channel#") {
                        if let ContainerData::Single(Data::Container(channel_props)) = channel_data {
                            let channel_name = key.trim_start_matches("Channel#").to_string();
                            let mut priority_order = None;
                            let mut channel_index = None;
                            let mut weight = None;

                            if let Some(ContainerData::Single(Data::Float(priority))) = channel_props.get("PriorityOrder") {
                                priority_order = Some(*priority);
                            }
                            if let Some(ContainerData::Single(Data::Int(index))) = channel_props.get("ChannelIndex") {
                                channel_index = Some(*index);
                            }
                            if let Some(ContainerData::Single(Data::Float(w))) = channel_props.get("Weight") {
                                weight = Some(*w);
                            }

                            channels.push(AnimationChannel {
                                name: channel_name,
                                priority_order,
                                channel_index,
                                weight,
                            });
                        }
                    }
                }
            }
        }

        // Parse animation files
        if let Some(ContainerData::Single(Data::Container(files))) = scene_data.get("Files") {
            for (key, file_data) in files {
                if key.starts_with("File#") {
                    if let ContainerData::Single(Data::Container(file_props)) = file_data {
                        let animation_name = key.trim_start_matches("File#").to_string();
                        let mut filename = String::new();
                        let mut metadata = None;

                        if let Some(ContainerData::Single(Data::String(fname))) = file_props.get("Filename") {
                            filename = fname.clone();
                        }

                        if let Some(ContainerData::Single(Data::Container(meta))) = file_props.get("MetaData") {
                            metadata = Some(meta.clone());
                        }

                        animations.push(AnimationInfo {
                            name: animation_name,
                            filename,
                            metadata,
                        });
                    }
                }
            }
        }

        Ok(AnimationData {
            version,
            model_filename,
            channels,
            animations,
        })
    }

    pub fn get_animation_names(&self) -> Vec<String> {
        if let Some(animation_data) = &self.animation_data {
            animation_data.animations.iter()
                .map(|anim| anim.name.clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_animation_info(&self, name: &str) -> Option<&AnimationInfo> {
        if let Some(animation_data) = &self.animation_data {
            animation_data.animations.iter()
                .find(|anim| anim.name == name)
        } else {
            None
        }
    }

    pub fn get_bent_file_path(&self) -> Option<&PathBuf> {
        self.current_bent_path.as_ref()
    }

    pub fn find_corresponding_bent_file<P: AsRef<Path>>(oct_path: P) -> Option<PathBuf> {
        let oct_path = oct_path.as_ref();
        let bent_path = oct_path.with_extension("bent");
        
        if bent_path.exists() {
            Some(bent_path)
        } else {
            None
        }
    }

pub fn extract_textures(&mut self, game_type: &GameType) -> anyhow::Result<()> {
    self.extracted_textures.clear();
    
    // Only extract textures for supported games
    let supported_games = [
        GameType::ToyShit3,
        GameType::Cars2Arcade,
        GameType::Cars2TheVideoGame,
    ];
    
    if !supported_games.contains(game_type) {
        return Ok(());
    }

    // Clone the scene data to avoid borrow issues
    let scene_data = if let Some(scene_data) = &self.current_scene {
        scene_data.clone()
    } else {
        return Ok(());
    };
    
    self.find_and_extract_textures(&scene_data, Path::new("extracted_textures"))?;
    
    Ok(())
}

    const TEXTURE_PREFIX: &str = "Texture#";
    const PATH_KEY: &str = "SourceFilePath";
    const DATA_KEY: &str = "Data";

    fn find_and_extract_textures(
        &mut self,
        data: &IndexMap<String, ContainerData>,
        output_path: &Path,
    ) -> anyhow::Result<()> {
        for (key, data) in data {
            match data {
                ContainerData::Single(Data::Container(container)) => {
                    if key.starts_with(Self::TEXTURE_PREFIX) {
                        if let (
                            Some(ContainerData::Single(Data::String(path))),
                            Some(ContainerData::Single(Data::Binary(data))),
                        ) = (container.get(Self::PATH_KEY), container.get(Self::DATA_KEY))
                        {
                            let out = output_path
                                .join(path.replace('\\', std::path::MAIN_SEPARATOR_STR))
                                .with_extension("dds");
                            
                            if let Some(parent) = out.parent() {
                                if !parent.exists() {
                                    fs::create_dir_all(parent)?;
                                }
                            }

                            // Store texture info
                            self.extracted_textures.push(TextureInfo {
                                name: path.clone(),
                                path: out.clone(),
                                data: data.clone(),
                            });
                        }
                    }

                    self.find_and_extract_textures(container, output_path)?;
                }
                ContainerData::Single(_) => {}
                _ => {} // Skip multiple container data
            }
        }

        Ok(())
    }

    pub fn has_scene_loaded(&self) -> bool {
        self.current_scene.is_some()
    }

    pub fn has_animation_data(&self) -> bool {
        self.animation_data.is_some()
    }

    pub fn has_textures(&self) -> bool {
        !self.extracted_textures.is_empty()
    }

    pub fn clear(&mut self) {
        self.current_scene = None;
        self.extracted_textures.clear();
        self.endian = None;
        self.animation_data = None;
        self.current_bent_path = None;
    }
}

// BinRead implementation for RawNode
impl BinRead for RawNode {
    type Args<'a> = &'a [String];

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let header_data: u16 = reader.read_type(endian)?;
        let header = NodeHeader::from(header_data);

        let key_idx: u16 = reader.read_type(endian)?;
        let key = &args[key_idx as usize];

        let name = if header.name() {
            let name_idx: u16 = reader.read_type(endian)?;
            Some(args[name_idx as usize].clone())
        } else {
            None
        };

        let level = header.level();

        let len_size = header.len_size() as usize + 1;
        let int_site = header.int_size() as usize + 1;

        let node = Node {
            id: match name {
                Some(name) => format!("{}#{}", key.clone(), name),
                None => key.clone(),
            },
            data: match (header.data_type(), header.r#type()) {
                (DataType::None, Type::Container) => NodeData::Container(vec![]),

                (DataType::String, Type::Scalar) => NodeData::String({
                    let idx: u16 = reader.read_type(endian)?;
                    args[idx as usize].clone()
                }),
                (DataType::String, Type::Vec) => NodeData::StringVec({
                    let len = read_u32(reader, endian, len_size)? as usize;
                    let mut vec = Vec::with_capacity(len);
                    for _ in 0..len {
                        let idx: u16 = reader.read_type(endian)?;
                        vec.push(args[idx as usize].clone());
                    }
                    vec
                }),

                (DataType::Float, Type::Scalar) => NodeData::Float(reader.read_type(endian)?),
                (DataType::Float, Type::Vec) => NodeData::FloatVec({
                    let len = read_u32(reader, endian, len_size)? as usize;
                    let mut vec = Vec::with_capacity(len);
                    for _ in 0..len {
                        vec.push(reader.read_type(endian)?);
                    }
                    vec
                }),
                (DataType::Int, Type::Scalar) => NodeData::Int(read_i32(reader, endian, int_site)?),
                (DataType::Int, Type::Vec) => NodeData::IntVec({
                    let len = read_u32(reader, endian, len_size)? as usize;
                    let mut vec = Vec::with_capacity(len);
                    for _ in 0..len {
                        vec.push(read_i32(reader, endian, int_site)?);
                    }
                    vec
                }),

                (DataType::Binary, Type::Scalar) => {
                    let len = read_u32(reader, endian, len_size)? as usize;
                    let mut vec = Vec::with_capacity(len);
                    for _ in 0..len {
                        vec.push(reader.read_type(endian)?);
                    }

                    // special case, uuids are encoded as binary
                    if len == 16 && key == "Uuid" {
                        let mut bytes: [u8; 16] = [0; 16];
                        bytes.copy_from_slice(vec.as_slice());

                        let uuid = match endian {
                            Endian::Big => Uuid::from_bytes(bytes),
                            Endian::Little => Uuid::from_bytes_le(bytes),
                        };

                        NodeData::Uuid(uuid)
                    } else {
                        NodeData::Binary(vec)
                    }
                }

                x => unimplemented!("{:?}", x),
            },
        };

        Ok(RawNode { level, node })
    }
}

// BinWrite implementation for RawNode
impl BinWrite for RawNode {
    type Args<'a> = &'a [String];

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        let mut len_size = 1;
        let mut int_size = 1;

        let (data_type, r#type) = match &self.node.data {
            NodeData::Container(_) => (DataType::None, Type::Container),
            NodeData::String(_) => (DataType::String, Type::Scalar),
            NodeData::StringVec(data) => {
                let len = data.len();
                len_size = get_u32_size(len as u32);
                (DataType::String, Type::Vec)
            }
            NodeData::Float(_) => (DataType::Float, Type::Scalar),
            NodeData::FloatVec(data) => {
                let len = data.len();
                len_size = get_u32_size(len as u32);
                (DataType::Float, Type::Vec)
            }
            NodeData::Int(data) => {
                int_size = get_i32_size(*data);
                (DataType::Int, Type::Scalar)
            }
            NodeData::IntVec(data) => {
                let len = data.len();
                len_size = get_u32_size(len as u32);
                int_size = data.iter().map(|x| get_i32_size(*x)).max().unwrap_or(1);
                (DataType::Int, Type::Vec)
            }
            NodeData::Binary(data) => {
                let len = data.len();
                len_size = get_u32_size(len as u32);
                (DataType::Binary, Type::Scalar)
            }
            NodeData::Uuid(_) => (DataType::Binary, Type::Scalar),
        };

        let key;
        let name;

        if let Some((k, n)) = self.node.id.split_once('#') {
            key = find_string_index(args, k);
            name = Some(find_string_index(args, n));
        } else {
            key = find_string_index(args, &self.node.id);
            name = None;
        }

        let mut header = NodeHeader::new();
        header.set_type(r#type);
        header.set_name(name.is_some());
        header.set_data_type(data_type);
        header.set_len_size(len_size - 1);
        header.set_int_size(int_size - 1);
        header.set_level(self.level);

        let header: u16 = header.into();

        writer.write_type(&header, endian)?;
        writer.write_type(&key, endian)?;
        if let Some(name) = name {
            writer.write_type(&name, endian)?;
        }

        match &self.node.data {
            NodeData::Container(_) => {}
            NodeData::String(data) => writer.write_type(&find_string_index(args, data), endian)?,
            NodeData::StringVec(data) => {
                write_u32(writer, data.len() as u32, endian, len_size as usize)?;
                for x in data {
                    writer.write_type(&find_string_index(args, x), endian)?;
                }
            }
            NodeData::Float(data) => writer.write_type(data, endian)?,
            NodeData::FloatVec(data) => {
                write_u32(writer, data.len() as u32, endian, len_size as usize)?;
                for x in data {
                    writer.write_type(x, endian)?;
                }
            }
            NodeData::Int(data) => {
                write_i32(writer, *data, endian, int_size as usize)?;
            }
            NodeData::IntVec(data) => {
                write_u32(writer, data.len() as u32, endian, len_size as usize)?;
                for x in data {
                    write_i32(writer, *x, endian, int_size as usize)?;
                }
            }
            NodeData::Binary(data) => {
                write_u32(writer, data.len() as u32, endian, len_size as usize)?;
                for x in data {
                    writer.write_type(x, endian)?;
                }
            }
            NodeData::Uuid(uuid) => {
                writer.write_type(&16u8, endian)?;
                let bytes = match endian {
                    Endian::Big => *uuid.as_bytes(),
                    Endian::Little => uuid.to_bytes_le(),
                };
                writer.write_all(&bytes)?;
            }
        };

        Ok(())
    }
}

// Helper functions
fn find_string_index(strings: &[String], string: &str) -> u16 {
    strings.iter().position(|s| s == string).unwrap_or(0) as u16
}

const fn get_u32_size(i: u32) -> u8 {
    let actual_bits = 32 - i.leading_zeros();
    let bytes_used = actual_bits / 8;
    let bits_remaining = actual_bits % 8;

    (if bits_remaining > 0 {
        bytes_used + 1
    } else if i == 0 {
        1
    } else {
        bytes_used
    }) as u8
}

const fn get_i32_size(i: i32) -> u8 {
    let actual_bits = 32 - i.abs().leading_zeros() + 1; // +1 for sign bit
    let bytes_used = actual_bits / 8;
    let bits_remaining = actual_bits % 8;

    (if bits_remaining > 0 {
        bytes_used + 1
    } else {
        bytes_used
    }) as u8
}

fn read_u32<R: Read + Seek>(reader: &mut R, endian: Endian, len: usize) -> binrw::BinResult<u32> {
    if len > 4 {
        return Err(binrw::Error::AssertFail {
            pos: reader.stream_position()?,
            message: "Len may not be greater than 4.".to_string(),
        });
    }

    let mut buf = [0u8; 4];
    Ok(match endian {
        Endian::Big => {
            reader.read_exact(&mut buf[4 - len..])?;
            u32::from_be_bytes(buf)
        }
        Endian::Little => {
            reader.read_exact(&mut buf[..len])?;
            u32::from_le_bytes(buf)
        }
    })
}

fn write_i32<W: Write + Seek>(
    write: &mut W,
    data: i32,
    endian: Endian,
    len: usize,
) -> binrw::BinResult<()> {
    match endian {
        Endian::Big => {
            let buf = data.to_be_bytes();
            for byte in buf.iter().skip(4 - len) {
                write.write_be(byte)?;
            }
        }
        Endian::Little => {
            let buf = data.to_le_bytes();
            for byte in buf.iter().take(len) {
                write.write_le(byte)?
            }
        }
    }

    Ok(())
}

fn write_u32<W: Write + Seek>(
    write: &mut W,
    data: u32,
    endian: Endian,
    len: usize,
) -> binrw::BinResult<()> {
    match endian {
        Endian::Big => {
            let buf = data.to_be_bytes();
            for item in buf.iter().skip(4 - len) {
                write.write_be(item)?;
            }
        }
        Endian::Little => {
            let buf = data.to_le_bytes();
            for item in buf.iter().take(len) {
                write.write_le(item)?
            }
        }
    }

    Ok(())
}

fn read_i32<R: Read + Seek>(reader: &mut R, endian: Endian, len: usize) -> binrw::BinResult<i32> {
    let data = read_u32(reader, endian, len)?;
    let bit_size = len as u32 * 8;
    let neg_mask = 1 << (bit_size - 1);

    if data & neg_mask == neg_mask {
        let mask = u32::MAX ^ (neg_mask - 1);
        Ok((data | mask) as i32)
    } else {
        Ok(data as i32)
    }
}

// Conversion implementations
impl TryFrom<NodeData> for Data {
    type Error = anyhow::Error;

    fn try_from(node_data: NodeData) -> Result<Self, Self::Error> {
        Ok(match node_data {
            NodeData::Container(child) => {
                let mut childs = IndexMap::new();
                for node in child {
                    if childs.contains_key(&node.id) {
                        let data = childs.swap_remove(&node.id).unwrap();
                        match data {
                            ContainerData::Single(first) => childs.insert(
                                node.id,
                                ContainerData::Multiple(vec![first, node.data.try_into()?]),
                            ),
                            ContainerData::Multiple(mut list) => {
                                list.push(node.data.try_into()?);
                                childs.insert(node.id, ContainerData::Multiple(list))
                            }
                        };
                    } else {
                        childs.insert(node.id, ContainerData::Single(node.data.try_into()?));
                    }
                }
                Data::Container(childs)
            }
            NodeData::String(str) => Data::String(str),
            NodeData::StringVec(str_vec) => Data::StringVec(str_vec),
            NodeData::Float(str_vec) => Data::Float(str_vec),
            NodeData::FloatVec(str_vec) => Data::FloatVec(str_vec),
            NodeData::Int(str_vec) => Data::Int(str_vec),
            NodeData::IntVec(str_vec) => Data::IntVec(str_vec),
            NodeData::Binary(str_vec) => Data::Binary(str_vec),
            NodeData::Uuid(uuid) => Data::Uuid(uuid),
        })
    }
}

impl From<Data> for NodeData {
    fn from(value: Data) -> Self {
        match value {
            Data::Container(child) => {
                let mut childs = Vec::with_capacity(child.len());
                for (id, data) in child {
                    let n = match data {
                        ContainerData::Single(x) => vec![x],
                        ContainerData::Multiple(x) => x,
                    };
                    for data in n {
                        childs.push(Node {
                            id: id.clone(),
                            data: data.into(),
                        });
                    }
                }
                NodeData::Container(childs)
            }
            Data::String(data) => NodeData::String(data),
            Data::StringVec(data) => NodeData::StringVec(data),
            Data::Float(data) => NodeData::Float(data),
            Data::FloatVec(data) => NodeData::FloatVec(data),
            Data::Int(data) => NodeData::Int(data),
            Data::IntVec(data) => NodeData::IntVec(data),
            Data::Binary(data) => NodeData::Binary(data),
            Data::Uuid(data) => NodeData::Uuid(data),
        }
    }
}