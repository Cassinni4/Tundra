use eframe::egui;
use std::path::{Path, PathBuf};
use image::ImageFormat;

#[derive(Clone)]
pub struct TbodyTexture {
    pub name: String,
    pub texture_handle: Option<egui::TextureHandle>,
    pub dimensions: (u32, u32),
    pub file_path: PathBuf,
}

impl TbodyTexture {
    pub fn load_from_file(file_path: &Path, ctx: &egui::Context) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read(file_path)?;
        Self::load_from_bytes(&data, file_path, ctx)
    }

    pub fn load_from_bytes(data: &[u8], file_path: &Path, ctx: &egui::Context) -> Result<Self, Box<dyn std::error::Error>> {
        // TBODY files are actually DDS files, so we need to handle DDS format
        let img = image::load_from_memory_with_format(data, ImageFormat::Dds)?;
        let rgba = img.to_rgba8();
        let dimensions = (rgba.width(), rgba.height());
        
        let name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Create texture handle
        let pixels = rgba.as_flat_samples();
        let texture_handle = Some(ctx.load_texture(
            name.clone(),
            egui::ColorImage::from_rgba_unmultiplied(
                [dimensions.0 as usize, dimensions.1 as usize],
                pixels.as_slice(),
            ),
            Default::default(),
        ));

        Ok(TbodyTexture {
            name,
            texture_handle,
            dimensions,
            file_path: file_path.to_path_buf(),
        })
    }
}

pub struct TbodyViewer {
    pub textures: Vec<TbodyTexture>,
}

impl TbodyViewer {
    pub fn new() -> Self {
        Self {
            textures: Vec::new(),
        }
    }

    pub fn load_texture(&mut self, file_path: &Path, ctx: &egui::Context) -> Result<(), Box<dyn std::error::Error>> {
        let texture = TbodyTexture::load_from_file(file_path, ctx)?;
        self.textures.push(texture);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }

    pub fn show_ui(&self, ui: &mut egui::Ui, available_size: egui::Vec2) {
        if self.textures.is_empty() {
            ui.label("No textures loaded");
            return;
        }

        // Calculate layout based on available space and number of textures
        let texture_count = self.textures.len();
        let max_textures_per_row = (available_size.x / 200.0).max(1.0) as usize;
        let textures_per_row = texture_count.min(max_textures_per_row);
        let row_count = (texture_count + textures_per_row - 1) / textures_per_row;
        
        let texture_size = if textures_per_row > 0 {
            (available_size.x / textures_per_row as f32 * 0.9).min(200.0)
        } else {
            200.0
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in 0..row_count {
                ui.horizontal(|ui| {
                    for col in 0..textures_per_row {
                        let index = row * textures_per_row + col;
                        if index >= self.textures.len() {
                            break;
                        }

                        let texture = &self.textures[index];
                        ui.vertical(|ui| {
                            // Show texture name
                            ui.label(&texture.name);
                            
                            // Show texture
                            if let Some(texture_handle) = &texture.texture_handle {
                                let display_size = egui::Vec2::splat(texture_size);
                                ui.add(egui::Image::new(texture_handle)
                                    .max_size(display_size)
                                    .maintain_aspect_ratio(true));
                            } else {
                                ui.label("Failed to load texture");
                            }
                            
                            // Show dimensions
                            ui.label(format!("{}x{}", texture.dimensions.0, texture.dimensions.1));
                        });
                    }
                });
            }
        });
    }
}