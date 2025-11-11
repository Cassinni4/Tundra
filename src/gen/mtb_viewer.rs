use eframe::egui;
use std::path::{Path, PathBuf};
use super::mtb_reader::MtbFile;
use super::tbody_viewer::TbodyViewer;

pub struct MtbViewer {
    mtb_file: Option<MtbFile>,
    tbody_viewer: TbodyViewer,
    base_path: Option<PathBuf>,
    loaded_textures: bool,
}

impl MtbViewer {
    pub fn new() -> Self {
        Self {
            mtb_file: None,
            tbody_viewer: TbodyViewer::new(),
            base_path: None,
            loaded_textures: false,
        }
    }

    pub fn load_mtb_file(&mut self, file_path: &Path, ctx: &egui::Context) -> Result<(), Box<dyn std::error::Error>> {
        self.clear();
        
        let mtb_file = MtbFile::load_from_file(file_path)?;
        self.mtb_file = Some(mtb_file);
        self.base_path = file_path.parent().map(|p| p.to_path_buf());
        
        // Try to load associated textures
        self.load_associated_textures(ctx);
        
        Ok(())
    }

    pub fn load_tbody_file(&mut self, file_path: &Path, ctx: &egui::Context) -> Result<(), Box<dyn std::error::Error>> {
        self.clear();
        self.tbody_viewer.load_texture(file_path, ctx)?;
        self.loaded_textures = true;
        Ok(())
    }

    fn load_associated_textures(&mut self, ctx: &egui::Context) {
        if let Some(mtb_file) = &self.mtb_file {
            if let Some(base_path) = &self.base_path {
                for texture_info in &mtb_file.textures {
                    // ONLY search in the central textures folder
                    let textures_path = base_path.parent()
                        .and_then(|p| p.parent())
                        .map(|assets_dir| assets_dir.join("textures").join(&texture_info.tbody_filename))
                        .unwrap_or_default();
                    
                    if textures_path.exists() {
                        if let Ok(()) = self.tbody_viewer.load_texture(&textures_path, ctx) {
                            println!("Loaded texture: {} from {}", texture_info.tbody_filename, textures_path.display());
                        } else {
                            println!("Failed to load texture: {}", texture_info.tbody_filename);
                        }
                    } else {
                        println!("Texture not found in textures folder: {}", texture_info.tbody_filename);
                    }
                }
                self.loaded_textures = true;
            }
        }
    }

    pub fn clear(&mut self) {
        self.mtb_file = None;
        self.tbody_viewer.clear();
        self.base_path = None;
        self.loaded_textures = false;
    }

    pub fn has_content(&self) -> bool {
        self.mtb_file.is_some() || !self.tbody_viewer.textures.is_empty()
    }

    pub fn show_ui(&mut self, ui: &mut egui::Ui, available_size: egui::Vec2, _ctx: &egui::Context) {
        if !self.has_content() {
            ui.label("No MTB or TBODY file loaded");
            return;
        }

        // Show MTB file information if available
        if let Some(mtb_file) = &self.mtb_file {
            ui.heading("MTB Texture Links");
            ui.separator();
            
            ui.label(format!("File: {}", mtb_file.file_path.display()));
            ui.label(format!("Found {} texture references:", mtb_file.textures.len()));
            
            for texture_info in &mtb_file.textures {
                // Check if texture is loaded
                let is_loaded = self.tbody_viewer.textures
                    .iter()
                    .any(|t| t.name == texture_info.tbody_filename);
                
                ui.horizontal(|ui| {
                    ui.label("•");
                    ui.monospace(&texture_info.name);
                    ui.label("→");
                    ui.monospace(&texture_info.tbody_filename);
                    
                    if is_loaded {
                        ui.colored_label(egui::Color32::GREEN, "Loaded");
                    } else {
                        ui.colored_label(egui::Color32::RED, "Missing");
                    }
                });
                
                // Show search info for missing textures
                if !is_loaded {
                    ui.indent("missing_texture_info", |ui| {
                        ui.label("Expected location: assets/textures/");
                    });
                }
            }
            
            ui.separator();
        }

        // Show textures
        if !self.tbody_viewer.textures.is_empty() {
            if self.mtb_file.is_some() {
                ui.heading("Loaded Textures");
            }
            self.tbody_viewer.show_ui(ui, available_size);
        } else if self.loaded_textures {
            ui.label("No textures could be loaded. Make sure TBODY files are available in assets/textures/ folder.");
        }
    }
}