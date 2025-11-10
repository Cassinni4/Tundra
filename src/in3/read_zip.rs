use aes::cipher::{KeyIvInit, StreamCipher};
use binrw::BinRead;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

type Aes128CtrCipher = ctr::Ctr128BE<aes::Aes128>;

const DI3_KEY: [u8; 16] = [
    0x68, 0x1B, 0xBE, 0xEA, 0x63, 0x16, 0x01, 0x88, 
    0xF9, 0xB7, 0x94, 0x51, 0x04, 0xA5, 0x14, 0x99
];

const PSX_KEY: [u8; 16] = [
    0x7D, 0xDD, 0x6D, 0x92, 0xF3, 0xA4, 0x6A, 0xBA, 
    0xF0, 0x61, 0xEB, 0xC3, 0xC0, 0x1D, 0x7D, 0x88
];

#[derive(BinRead, Debug)]
#[brw(little)]
struct ZipLocalFileHeader {
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

pub struct DisneyInfinityZipReader;

impl DisneyInfinityZipReader {
    fn get_key(file_name: &str) -> &'static [u8; 16] {
        if file_name.to_lowercase().starts_with("psx_") {
            &PSX_KEY
        } else {
            &DI3_KEY
        }
    }

    fn create_cipher(key: &[u8; 16]) -> Aes128CtrCipher {
        Aes128CtrCipher::new_from_slices(key, &[0x00; 16]).unwrap()
    }

    fn decrypt_data(data: &mut [u8], key: &[u8; 16], bytes_to_decrypt: usize) {
        let mut cipher = Self::create_cipher(key);
        let bytes_to_decrypt = bytes_to_decrypt.min(data.len());
        cipher.apply_keystream(&mut data[..bytes_to_decrypt]);
    }

    pub fn is_disney_infinity_zip<P: AsRef<Path>>(zip_path: P) -> bool {
        let path = zip_path.as_ref();
        
        // Get file name from path for key selection
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        
        let key = Self::get_key(file_name);
        
        if let Ok(file) = std::fs::File::open(path) {
            let mut reader = std::io::BufReader::new(file);
            
            // Read and try to decrypt the header
            let mut header_data = vec![0u8; 4];
            if reader.read_exact(&mut header_data).is_ok() {
                let header_len = header_data.len();
                Self::decrypt_data(&mut header_data, key, header_len);
                
                // Check if it's the PK\xff\xff header
                return &header_data == b"PK\xff\xff";
            }
        }
        
        false
    }

    pub fn read_zip_contents<P: AsRef<Path>>(
        zip_path: P,
    ) -> Result<Vec<DisneyInfinityZipEntry>, Box<dyn std::error::Error>> {
        let path = zip_path.as_ref();
        
        // Get file name from path for key selection
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        
        let key = Self::get_key(file_name);
        
        let file = std::fs::File::open(path)?;
        let file_size = file.metadata()?.len();
        let mut reader = std::io::BufReader::new(file);
        
        println!("Reading Disney Infinity zip: {} (size: {} bytes)", file_name, file_size);
        
        // Read and decrypt the PK\xff\xff header
        let mut header_data = vec![0u8; 4];
        reader.read_exact(&mut header_data)?;
        let header_len = header_data.len();
        Self::decrypt_data(&mut header_data, key, header_len);
        
        if &header_data != b"PK\xff\xff" {
            return Err("Not a valid Disney Infinity 3.0 encrypted zip".into());
        }
        
        // Read number of files - but be careful about the value
        let mut files_count_data = vec![0u8; 4];
        reader.read_exact(&mut files_count_data)?;
        let files_count_len = files_count_data.len();
        Self::decrypt_data(&mut files_count_data, key, files_count_len);
        
        let files_count = u32::from_le_bytes(files_count_data.try_into().unwrap());
        
        // Sanity check: if files_count is ridiculously large, something went wrong
        // A reasonable upper limit would be file_size / 100 (average 100 bytes per file)
        let max_reasonable_files = (file_size / 100) as u32;
        if files_count > max_reasonable_files {
            println!("File count {} seems unreasonable for a {} byte file, limiting to {}", 
                     files_count, file_size, max_reasonable_files);
            // Let's try a different approach - read until we can't read any more entries
            return Self::read_zip_contents_manual(&mut reader, key, file_size);
        }
        
        println!("Found {} files in Disney Infinity zip", files_count);
        
        let mut entries = Vec::new();
        
        // Read the octane zip entries (name hashes and offsets)
        for i in 0..files_count {
            let mut entry_data = vec![0u8; 8]; // 4 bytes for hash + 4 bytes for offset
            if reader.read_exact(&mut entry_data).is_err() {
                println!("Failed to read entry {} of {}", i, files_count);
                break;
            }
            
            let entry_data_len = entry_data.len();
            Self::decrypt_data(&mut entry_data, key, entry_data_len);
            
            let name_mmh3 = u32::from_le_bytes(entry_data[0..4].try_into().unwrap());
            let header_offset = u32::from_le_bytes(entry_data[4..8].try_into().unwrap());
            
            // Skip obviously invalid offsets
            if header_offset as u64 >= file_size {
                println!("Skipping entry {}: offset {} beyond file size {}", i, header_offset, file_size);
                continue;
            }
            
            // Try to read the file header at this offset
            if let Some(entry) = Self::read_file_header(&mut reader, key, header_offset, file_size) {
                entries.push(entry);
            }
        }
        
        println!("Successfully read {} entries from Disney Infinity zip", entries.len());
        Ok(entries)
    }

    fn read_zip_contents_manual(
        reader: &mut std::io::BufReader<std::fs::File>,
        key: &[u8; 16],
        file_size: u64,
    ) -> Result<Vec<DisneyInfinityZipEntry>, Box<dyn std::error::Error>> {
        println!("Using manual reading method...");
        
        let mut entries = Vec::new();
        let mut entry_count = 0;
        
        // Try to read entries until we can't read any more
        loop {
            let mut entry_data = vec![0u8; 8]; // 4 bytes for hash + 4 bytes for offset
            if reader.read_exact(&mut entry_data).is_err() {
                break;
            }
            
            let entry_data_len = entry_data.len();
            Self::decrypt_data(&mut entry_data, key, entry_data_len);
            
            let name_mmh3 = u32::from_le_bytes(entry_data[0..4].try_into().unwrap());
            let header_offset = u32::from_le_bytes(entry_data[4..8].try_into().unwrap());
            
            // Stop if we get a zero offset (likely end of entries)
            if header_offset == 0 {
                break;
            }
            
            // Skip obviously invalid offsets
            if header_offset as u64 >= file_size {
                println!("Skipping entry {}: offset {} beyond file size {}", entry_count, header_offset, file_size);
                entry_count += 1;
                continue;
            }
            
            // Try to read the file header at this offset
            if let Some(entry) = Self::read_file_header(reader, key, header_offset, file_size) {
                entries.push(entry);
            }
            
            entry_count += 1;
            
            // Safety limit
            if entry_count > 10000 {
                println!("Reached safety limit of 10000 entries");
                break;
            }
        }
        
        println!("Manually read {} entries from Disney Infinity zip", entries.len());
        Ok(entries)
    }

    fn read_file_header(
        reader: &mut std::io::BufReader<std::fs::File>,
        key: &[u8; 16],
        header_offset: u32,
        file_size: u64,
    ) -> Option<DisneyInfinityZipEntry> {
        let current_pos = match reader.stream_position() {
            Ok(pos) => pos,
            Err(_) => return None,
        };
        
        if header_offset as u64 >= file_size {
            return None;
        }
        
        // Seek to the file header
        if reader.seek(SeekFrom::Start(header_offset as u64)).is_err() {
            let _ = reader.seek(SeekFrom::Start(current_pos));
            return None;
        }
        
        // Read the local file header (30 bytes)
        let mut header_data = vec![0u8; 30];
        if reader.read_exact(&mut header_data).is_err() {
            let _ = reader.seek(SeekFrom::Start(current_pos));
            return None;
        }
        
        // Decrypt the header (first 0x200 bytes)
        let header_data_len = 0x200.min(header_data.len());
        Self::decrypt_data(&mut header_data, key, header_data_len);
        
        // Parse the header
        let mut header_cursor = std::io::Cursor::new(&header_data);
        let header = match ZipLocalFileHeader::read(&mut header_cursor) {
            Ok(header) => header,
            Err(_) => {
                let _ = reader.seek(SeekFrom::Start(current_pos));
                return None;
            }
        };
        
        // Verify signature
        if header.signature != 0x04034b50 {
            let _ = reader.seek(SeekFrom::Start(current_pos));
            return None;
        }
        
        // Read file name
        let mut file_name_data = vec![0u8; header.file_name_length as usize];
        if reader.read_exact(&mut file_name_data).is_err() {
            let _ = reader.seek(SeekFrom::Start(current_pos));
            return None;
        }
        
        // Decrypt file name (first 0x200 bytes)
        let file_name_data_len = 0x200.min(file_name_data.len());
        Self::decrypt_data(&mut file_name_data, key, file_name_data_len);
        
        let file_name = String::from_utf8_lossy(&file_name_data).to_string();
        
        // Skip extra field
        let _ = reader.seek(SeekFrom::Current(header.extra_field_length as i64));
        
        println!("Found file: '{}' (offset: {}, size: {})", file_name, header_offset, header.compressed_size);
        
        // Restore original position
        let _ = reader.seek(SeekFrom::Start(current_pos));
        
        Some(DisneyInfinityZipEntry {
            name: file_name,
            is_directory: false,
            header_offset,
            compressed_size: header.compressed_size,
            uncompressed_size: header.uncompressed_size,
            compression_method: header.compression,
        })
    }

    pub fn extract_file<P: AsRef<Path>>(
        zip_path: P,
        entry: &DisneyInfinityZipEntry,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let path = zip_path.as_ref();
        
        // Get file name from path for key selection
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        
        let key = Self::get_key(file_name);
        
        let file = std::fs::File::open(path)?;
        let mut reader = std::io::BufReader::new(file);
        
        // Seek to the file data (header offset + header size + file name + extra field)
        let data_offset = entry.header_offset as u64 + 30 + entry.name.len() as u64;
        reader.seek(SeekFrom::Start(data_offset))?;
        
        // Read compressed data
        let mut compressed_data = vec![0u8; entry.compressed_size as usize];
        reader.read_exact(&mut compressed_data)?;
        
        // Decrypt only the first 0x200 bytes (unless it's a .dct file)
        let bytes_to_decrypt = if entry.name.to_lowercase().ends_with(".dct") {
            compressed_data.len()
        } else {
            0x200.min(compressed_data.len())
        };
        
        Self::decrypt_data(&mut compressed_data, key, bytes_to_decrypt);
        
        // Decompress if needed
        if entry.compression_method == 0 {
            // Store - no compression
            Ok(compressed_data)
        } else if entry.compression_method == 8 {
            // Deflate
            let mut decoder = flate2::read::DeflateDecoder::new(&compressed_data[..]);
            let mut decompressed_data = Vec::new();
            decoder.read_to_end(&mut decompressed_data)?;
            Ok(decompressed_data)
        } else {
            Err(format!("Unsupported compression method: {}", entry.compression_method).into())
        }
    }
}

#[derive(Debug, Clone)]
pub struct DisneyInfinityZipEntry {
    pub name: String,
    pub is_directory: bool,
    pub header_offset: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub compression_method: u16,
}