use binrw::{binrw, BinRead};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

// stolen from offsetting, mostly
fn map_bytes_to_string(data: Vec<u8>) -> Result<String, std::str::Utf8Error> {
  std::str::from_utf8(&data).map(|str_slice| str_slice.to_string())
}

fn map_string_to_bytes(string: &String) -> &[u8] {
  string.as_bytes()
}

#[derive(BinRead, Debug)]
#[brw(little)]
pub struct ZipLocalFileHeader {
    #[br(assert(signature == 0x04034b50, "Invalid local file header signature"))]
    pub signature: u32,
    pub version: u16,
    pub flags: u16,
    pub compression: u16,
    pub mod_time: u16,
    pub mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name_length: u16,
    pub extra_field_length: u16,
}

#[binrw]
#[brw(little, magic = b"PK\x01\x02")]
pub struct ZipDirEntry {
    pub version_made_by: u16,
    pub version_to_extract: u16,
    pub flags: u16,
    pub compression_type: u16,
    pub file_time: u16,
    pub file_date: u16,
    pub file_crc: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    #[br(temp)]
    #[bw(calc = file_name.as_bytes().len() as u16)]
    file_name_length: u16,
    #[br(temp)]
    #[bw(calc = file_extra_field.len() as u16)]
    file_extra_field_length: u16,
    #[br(temp)]
    #[bw(calc = file_comment.as_bytes().len() as u16)]
    file_comment_length: u16,
    pub disk_number_start: u16,
    pub internal_attributes: u16,
    pub external_attributes: u32,
    pub header_offset: u32,
    #[br(count = file_name_length, try_map = map_bytes_to_string)]
    #[bw(map = map_string_to_bytes)]
    pub file_name: String,
    #[br(count = file_extra_field_length)]
    pub file_extra_field: Vec<u8>,
    #[br(count = file_comment_length, try_map = map_bytes_to_string)]
    #[bw(map = map_string_to_bytes)]
    pub file_comment: String,
}

const ZIP_END_LOCATOR_SIZE: usize = 22;
const MD5_HEADER: [u8; 7] = [0x4B, 0x46, 0x13, 0x00, 0x4D, 0x44, 0x35];
const MD5_EXTRA_FIELD_SIZE: usize = MD5_HEADER.len() + 16;

#[binrw]
#[brw(little, magic = b"PK\x05\x06")]
pub struct ZipDirEndLocator {
    pub disk_number: u16,
    pub disk_start_number: u16,
    pub entries_on_disk: u16,
    pub entries_in_directory: u16,
    pub directory_size: u32,
    pub directory_offset: u32,
    #[br(temp)]
    #[bw(calc = comment.as_bytes().len() as u16)]
    comment_length: u16,
    #[br(count = comment_length, try_map = map_bytes_to_string)]
    #[bw(map = map_string_to_bytes)]
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub header_offset: u32,
    pub uncompressed_size: u32,
    pub compressed_size: u32,
    pub file_crc: u32,
    pub md5_hash: [u8; 16],
    pub file_name: String,
}

pub struct DrivenToWinZip;

impl DrivenToWinZip {
    pub fn read_zip_contents<P: AsRef<Path>>(
        zip_path: P,
    ) -> Result<Vec<ZipDirEntry>, Box<dyn std::error::Error>> {
        let path = zip_path.as_ref();

        let mut file = std::fs::File::open(zip_path)?;
        let mut file_len = file.metadata()?.len();
        let mut eocd_offset = None;

        for pos in (0..=file_len - 22).rev() {
            file.seek(SeekFrom::Start(pos))?;
            let mut buf = [0u8; 4];
            file.read_exact(&mut buf)?;
            let value = u32::from_le_bytes(buf);

            if value == 0x06054b50 as u32 {
                eocd_offset = Some(pos);
                println!("Found EOCD at {}", pos);
                break;
            }
        }

        let eocd_offset = match eocd_offset {
            Some(v) => v,
            None => return Err("EOCD not found".into())
        };

        file.seek(SeekFrom::Start(eocd_offset))?;

        let eocd: ZipDirEndLocator = ZipDirEndLocator::read(&mut file)?;
        let file_count = eocd.entries_in_directory as usize;
        file.seek(SeekFrom::Start(eocd.directory_offset as u64))?;

        let mut entries = Vec::with_capacity(file_count);
        for _ in 0..file_count {
            let entry = ZipDirEntry::read(&mut file)?;
            entries.push(entry);
        }

        Ok(entries)
    }

    pub fn try_zlib_deflate(compressed: &[u8], 
        expected: usize, 
        name: &str
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut decoder = flate2::read::ZlibDecoder::new(compressed);
        let mut decompressed_data = Vec::new();

        // Try zlib
        if decoder.read_to_end(&mut decompressed_data).is_ok() && decompressed_data.len() == expected {
            return Ok(decompressed_data);
        }

        // Try deflate if zlib fails
        decompressed_data.clear();
        let mut decoder = flate2::read::DeflateDecoder::new(compressed);
        if decoder.read_to_end(&mut decompressed_data).is_ok() && decompressed_data.len() == expected {
            println!("Successfully decompressed {}", name);
            return Ok(decompressed_data);
        } else {
            return Err(format!("Failed to decompress {}", name).into());
        }
    }

    pub fn extract_zip_file(
        entry: ZipDirEntry, 
        file: &mut File
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        file.seek(SeekFrom::Start(entry.header_offset as u64))?;

        let _local_header = ZipLocalFileHeader::read(file)?;

        let mut compressed_data = vec![0; entry.compressed_size as usize];
        file.read_exact(&mut compressed_data)?;

        let decompressed_data = Self::try_zlib_deflate(&compressed_data[..], entry.uncompressed_size as usize, &entry.file_name)?;

        Ok(decompressed_data)
    }
}
