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
    pub is_ui_mtb: bool,
}

impl MtbFile {
    pub fn parse_from_bytes(data: &[u8], file_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let mut textures = Vec::new();
        let mut is_ui_mtb = false;

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
                    is_ui_mtb,
                });
            }
        };

        // Skip past TEXB header (4 bytes)
        let mut cursor = texb_start + 4;

        // Debug the TEXB section
        Self::debug_texb_section(data, texb_start);

        // Check if this is a UI MTB by looking for MATP header
        let matp_header = b"MATP";
        let has_matp = data[texb_start..].windows(4).any(|window| window == matp_header);
        
        if has_matp {
            println!("Detected normal MTB (has MATP section)");
            textures.extend_from_slice(&Self::parse_normal_texb_section(data, cursor));
        } else {
            println!("Detected UI MTB (no MATP section)");
            is_ui_mtb = true;
            textures.extend_from_slice(&Self::parse_ui_texb_section(data, cursor));
        }

        println!("Extracted {} valid textures from TEXB section", textures.len());

        Ok(MtbFile {
            textures,
            file_path: file_path.to_path_buf(),
            is_ui_mtb,
        })
    }

    fn parse_normal_texb_section(data: &[u8], start: usize) -> Vec<MtbTextureInfo> {
        let mut textures = Vec::new();
        let mut cursor = start;
        let matp_header = b"MATP";

        println!("Parsing normal MTB TEXB section");

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

    fn parse_ui_texb_section(data: &[u8], start: usize) -> Vec<MtbTextureInfo> {
        let mut textures = Vec::new();
        let mut cursor = start;

        println!("Parsing UI MTB TEXB section");

        // Read texture count (little endian u32)
        if cursor + 4 > data.len() {
            return textures;
        }
        let texture_count = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]) as usize;
        cursor += 4;

        println!("UI Texture count: {}", texture_count);

        // Read section size (little endian u32)
        if cursor + 4 > data.len() {
            return textures;
        }
        let section_size = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]);
        cursor += 4;
        println!("UI Section size: 0x{:08X} ({} bytes)", section_size, section_size);

        // Read actual texture count for UI MTB
        if cursor + 4 > data.len() {
            return textures;
        }
        let actual_texture_count = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]) as usize;
        cursor += 4;
        println!("UI Actual texture count: {}", actual_texture_count);

        // The next bytes are the material name string length (u32) followed by the string
        if cursor + 4 > data.len() {
            return textures;
        }
        let string_length = u32::from_le_bytes([data[cursor], data[cursor + 1], data[cursor + 2], data[cursor + 3]]) as usize;
        cursor += 4;
        println!("UI Material name length: {}", string_length);

        // Read the material name string
        if cursor + string_length > data.len() {
            println!("Not enough data for material name (need {} bytes)", string_length);
            return textures;
        }
        
        let string_bytes = &data[cursor..cursor + string_length];
        let material_name = String::from_utf8_lossy(string_bytes);
        println!("UI Material name: '{}' (length: {})", material_name, string_length);
    
        // Skip the string
        cursor += string_length;

        // Skip any padding to align to 4-byte boundary
        while cursor % 4 != 0 && cursor < data.len() {
            cursor += 1;
        }

        println!("UI Texture data starts at: 0x{:X}", cursor);

        // UI MTB texture entries are 8 bytes each
        for i in 0..actual_texture_count {
            // Safety check - make sure we have enough data
            if cursor + 8 > data.len() {
                println!("Not enough data for UI texture entry {} (need 8 bytes, have {} bytes)", 
                    i, data.len() - cursor);
                break;
            }

            let texture_bytes = &data[cursor..cursor + 8];

            // Convert the 8 bytes to hex filename
            let hex_filename = texture_bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();

            let tbody_filename = format!("{}.tbody", hex_filename);

            // Create a readable name from the hex for display
            let name = format!("texture_{}", i);

            println!("UI Texture {} at 0x{:X}: bytes {:02X?} -> {}", 
                i, cursor, texture_bytes, tbody_filename);

            textures.push(MtbTextureInfo {
                name,
                tbody_filename,
                offset: cursor,
            });
        
            cursor += 8;
        }

        textures
    }

    fn debug_texb_section(data: &[u8], texb_start: usize) {
        println!("=== TEXB Section Debug ===");
        
        // Show bytes from TEXB header to MATP header or reasonable limit
        let matp_header = b"MATP";
        let mut section_end = texb_start + 200; // Default limit
        
        // Check if this has MATP header
        let has_matp = data[texb_start..].windows(4).any(|window| window == matp_header);
        
        if has_matp {
            for i in texb_start..data.len().min(texb_start + 500) {
                if i + 4 <= data.len() && &data[i..i + 4] == matp_header {
                    section_end = i;
                    println!("Found MATP header at: 0x{:X}", i);
                    break;
                }
            }
        } else {
            // For UI MTB, show more data since there's no MATP header
            section_end = (texb_start + 300).min(data.len());
            println!("No MATP header found (UI MTB)");
        }

        println!("TEXB section from 0x{:X} to 0x{:X} (data len: 0x{:X})", texb_start, section_end, data.len());
        
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