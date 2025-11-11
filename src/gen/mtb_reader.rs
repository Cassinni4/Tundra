use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtbTextureInfo {
    pub name: String,
    pub tbody_filename: String,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtbFile {
    pub textures: Vec<MtbTextureInfo>,
    pub file_path: PathBuf,
}

impl MtbFile {
    pub fn parse_from_bytes(data: &[u8], file_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let mut textures = Vec::new();

        // Find the TEXB header
        let texb_header = b"TEXB";
        let mut cursor = 0;
        let mut texb_start = None;

        // Search for TEXB header
        while cursor <= data.len().saturating_sub(4) {
            if &data[cursor..cursor + 4] == texb_header {
                texb_start = Some(cursor);
                break;
            }
            cursor += 1;
        }

        let texb_start = match texb_start {
            Some(start) => {
                println!("Found TEXB header at offset: 0x{:X}", start);
                start
            },
            None => {
                println!("TEXB header not found!");
                return Ok(MtbFile {
                    textures,
                    file_path: file_path.to_path_buf(),
                });
            }
        };

        // Skip past TEXB header (4 bytes)
        let mut cursor = texb_start + 4;

        // Debug the TEXB section
        Self::debug_texb_section(data, texb_start);

        // Parse the TEXB section structure
        textures.extend_from_slice(&Self::parse_texb_section_corrected(data, cursor));

        println!("Extracted {} valid textures from TEXB section", textures.len());

        Ok(MtbFile {
            textures,
            file_path: file_path.to_path_buf(),
        })
    }

    fn parse_texb_section_corrected(data: &[u8], start: usize) -> Vec<MtbTextureInfo> {
        let mut textures = Vec::new();
        let mut cursor = start;
        let matp_header = b"MATP";

        println!("Parsing TEXB section with corrected structure");

        // Read texture count (little endian u32)
        if cursor + 4 > data.len() {
            return textures;
        }
        let texture_count = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]) as usize;
        cursor += 4;

        println!("Texture count: {}", texture_count);

        // Read section size (little endian u32)
        if cursor + 4 > data.len() {
            return textures;
        }
        let section_size = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]);
        cursor += 4;
        println!("Section size: 0x{:08X} ({} bytes)", section_size, section_size);

        // Read another field (might be actual texture count or offsets)
        if cursor + 4 > data.len() {
            return textures;
        }
        let field3 = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]);
        cursor += 4;
        println!("Field 3: 0x{:08X}", field3);

        // Skip padding or unknown data (4 bytes of zeros)
        if cursor + 4 > data.len() {
            return textures;
        }
        let padding = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]);
        println!("Padding: 0x{:08X}", padding);
        cursor += 4;

        // Now we should be at the actual texture entries
        // Each texture entry appears to be 12 bytes:
        // - 8 bytes: texture identifier (raw bytes for hex filename)
        // - 4 bytes: FF FF FF FF (separator)
        
        let actual_texture_count = field3 as usize; // Use field3 as the actual count
        
        println!("Looking for {} texture entries starting at 0x{:X}", actual_texture_count, cursor);

        for i in 0..actual_texture_count {
            // Stop if we hit MATP header or run out of data
            if cursor + 4 <= data.len() && &data[cursor..cursor + 4] == matp_header {
                println!("Reached MATP header after {} textures", i);
                break;
            }

            if cursor + 12 > data.len() {
                println!("Not enough data for texture entry {}", i);
                break;
            }

            // Check if we have the pattern: 8 bytes + FF FF FF FF
            let has_ffff_pattern = data[cursor + 8..cursor + 12] == [0xFF, 0xFF, 0xFF, 0xFF];
            
            let texture_bytes = &data[cursor..cursor + 8];
            
            // Convert the 8 bytes to hex filename
            let hex_filename = texture_bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            
            let tbody_filename = format!("{}.tbody", hex_filename);
            
            // Create a readable name
            let name: String = texture_bytes
                .iter()
                .map(|&b| if b >= 0x20 && b <= 0x7E { b as char } else { '.' })
                .collect();
            
            println!("Texture {}: bytes {:02X?} -> {} -> {} (FFFF pattern: {})", 
                i, texture_bytes, name, tbody_filename, has_ffff_pattern);
            
            textures.push(MtbTextureInfo {
                name,
                tbody_filename,
                offset: cursor,
            });
            
            cursor += 12;
        }

        textures
    }

    fn debug_texb_section(data: &[u8], texb_start: usize) {
        println!("=== TEXB Section Debug ===");
        
        // Show bytes from TEXB header to MATP header or reasonable limit
        let matp_header = b"MATP";
        let mut section_end = texb_start + 200; // Default limit
        for i in texb_start..data.len().min(texb_start + 500) {
            if i + 4 <= data.len() && &data[i..i + 4] == matp_header {
                section_end = i;
                println!("Found MATP header at: 0x{:X}", i);
                break;
            }
        }

        println!("TEXB section from 0x{:X} to 0x{:X}", texb_start, section_end);
        
        for i in (texb_start..section_end).step_by(16) {
            let line_end = (i + 16).min(section_end);
            let hex: Vec<String> = data[i..line_end].iter().map(|b| format!("{:02X}", b)).collect();
            let ascii: String = data[i..line_end].iter().map(|&b| 
                if b >= 0x20 && b <= 0x7E { b as char } else { '.' }
            ).collect();
            
            println!("0x{:06X}: {:48} {}", i, hex.join(" "), ascii);
        }
        
        println!("=== End Debug ===");
    }

    pub fn load_from_file(file_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read(file_path)?;
        Self::parse_from_bytes(&data, file_path)
    }
}