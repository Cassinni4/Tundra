use eframe::egui;
use eframe::egui::Widget;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
enum GameType {
    DisneyInfinity30,
    Cars2TheVideoGame,
    Cars2Arcade,
    Cars3DrivenToWin,
}

impl GameType {
    fn as_str(&self) -> &'static str {
        match self {
            GameType::DisneyInfinity30 => "Disney Infinity 3.0",
            GameType::Cars2TheVideoGame => "Cars 2: The Video Game",
            GameType::Cars2Arcade => "Cars 2 Arcade",
            GameType::Cars3DrivenToWin => "Cars 3: Driven To Win",
        }
    }

    fn expected_executable(&self) -> &'static str {
        match self {
            GameType::DisneyInfinity30 => "DisneyInfinity3.exe",
            GameType::Cars2TheVideoGame => "Game-Cars.exe",
            GameType::Cars2Arcade => "sdaemon.exe",
            GameType::Cars3DrivenToWin => "Cars3.exe",
        }
    }

    fn all() -> Vec<Self> {
        vec![
            GameType::DisneyInfinity30,
            GameType::Cars2TheVideoGame,
            GameType::Cars2Arcade,
            GameType::Cars3DrivenToWin,
        ]
    }

    fn supports_zip_browsing(&self) -> bool {
        matches!(self, GameType::Cars2TheVideoGame | GameType::Cars2Arcade)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GameConfig {
    executable_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppState {
    selected_game: Option<GameType>,
    game_configs: HashMap<GameType, GameConfig>,
    current_step: AppStep,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum AppStep {
    GameSelection,
    FileSelection,
    Editor,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            selected_game: None,
            game_configs: HashMap::new(),
            current_step: AppStep::GameSelection,
        }
    }
}

#[derive(Debug, Clone)]
struct FileEntry {
    path: PathBuf,
    is_directory: bool,
    is_zip: bool,
    children: Vec<FileEntry>,
    zip_contents_loaded: bool,
}

impl FileEntry {
    fn new(path: PathBuf, is_directory: bool) -> Self {
        let is_zip = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);

        Self {
            path,
            is_directory,
            is_zip,
            children: Vec::new(),
            zip_contents_loaded: false,
        }
    }
}

#[derive(Debug, Clone)]
struct ZipEntry {
    name: String,
    is_directory: bool,
}

struct TundraEditor {
    state: AppState,
    pending_file_selection: bool,
    selected_file: Option<PathBuf>,
    file_tree: Vec<FileEntry>,
    expanded_folders: std::collections::HashSet<PathBuf>,
    file_icons: HashMap<String, egui::TextureHandle>,
    config_path: PathBuf,
}

impl TundraEditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config_path = PathBuf::from("tundra_config.json");
        
        let mut app = Self {
            state: AppState::default(),
            pending_file_selection: false,
            selected_file: None,
            file_tree: Vec::new(),
            expanded_folders: std::collections::HashSet::new(),
            file_icons: HashMap::new(),
            config_path,
        };

        // Load file icons
        app.load_file_icons(cc);

        // Try to load state from JSON file
        app.load_from_json();

        app
    }

    fn load_from_json(&mut self) {
        if let Ok(file_content) = fs::read_to_string(&self.config_path) {
            match serde_json::from_str::<AppState>(&file_content) {
                Ok(loaded_state) => {
                    self.state = loaded_state;
                    println!("Loaded state from JSON with {} configured games", self.state.game_configs.len());
                    
                    // If we have a selected game with a valid path, scan its assets folder
                    if let Some(game_type) = &self.state.selected_game {
                        if let Some(config) = self.state.game_configs.get(game_type) {
                            if self.validate_executable(game_type, &config.executable_path) {
                                let path = config.executable_path.clone();
                                self.scan_assets_folder(&path);
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to parse config file: {}", e);
                }
            }
        } else {
            println!("No config file found at {}", self.config_path.display());
        }
    }

    fn load_file_icons(&mut self, cc: &eframe::CreationContext<'_>) {
        let icon_files = [
            ("bnk", "src/art/bnk.png"),
            ("lua", "src/art/lua.png"),
            ("wem", "src/art/wem.png"),
            ("zip", "src/art/zip.png"),
        ];

        for (extension, path) in icon_files.iter() {
            if let Ok(image_data) = std::fs::read(path) {
                if let Ok(image) = image::load_from_memory(&image_data) {
                    let size = [16, 16];
                    let image = image.resize_exact(
                        size[0],
                        size[1],
                        image::imageops::FilterType::Lanczos3,
                    );
                    let rgba = image.to_rgba8();
                    let pixels = rgba.as_flat_samples();
                    let texture = cc.egui_ctx.load_texture(
                        format!("icon_{}", extension),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [size[0] as usize, size[1] as usize],
                            pixels.as_slice(),
                        ),
                        Default::default(),
                    );
                    self.file_icons.insert(extension.to_string(), texture);
                } else {
                    eprintln!("Failed to load icon: {}", path);
                }
            } else {
                eprintln!("Failed to read icon file: {}", path);
            }
        }
    }

    fn get_file_icon(&self, file_path: &Path) -> Option<&egui::TextureHandle> {
        if let Some(extension) = file_path.extension() {
            if let Some(ext_str) = extension.to_str() {
                return self.file_icons.get(ext_str);
            }
        }
        None
    }

    fn save_state(&self) {
        // Save to JSON file
        if let Ok(serialized) = serde_json::to_string_pretty(&self.state) {
            if let Err(e) = fs::write(&self.config_path, serialized) {
                eprintln!("Failed to save config to JSON file: {}", e);
            } else {
                println!("Saved state to {}", self.config_path.display());
            }
        } else {
            eprintln!("Failed to serialize state to JSON");
        }
    }

    fn open_file_dialog(&mut self) {
        self.pending_file_selection = true;
    }

    fn handle_file_dialog(&mut self) {
        if self.pending_file_selection {
            if let Some(game_type) = self.state.selected_game.clone() {
                if let Some(file_path) = rfd::FileDialog::new()
                    .set_title(&format!("Select {} executable", game_type.as_str()))
                    .add_filter("Executable", &["exe"])
                    .pick_file()
                {
                    let config = GameConfig {
                        executable_path: file_path.clone(),
                    };
                    self.state.game_configs.insert(game_type.clone(), config);
                    
                    // Save state immediately when a new executable is selected
                    self.save_state();
                    
                    // Automatically go to editor if valid executable
                    if self.validate_executable(&game_type, &file_path) {
                        self.scan_assets_folder(&file_path);
                        self.state.current_step = AppStep::Editor;
                        println!("Valid executable selected for {}, opening editor", game_type.as_str());
                    } else {
                        println!("File selected for {} but name doesn't match expected", game_type.as_str());
                        // Stay in file selection mode for invalid files
                    }
                }
            }
            self.pending_file_selection = false;
        }
    }

    fn validate_executable(&self, game_type: &GameType, path: &Path) -> bool {
        if let Some(file_name) = path.file_name() {
            if let Some(name) = file_name.to_str() {
                return name.eq_ignore_ascii_case(game_type.expected_executable());
            }
        }
        false
    }

    fn get_game_path(&self, game_type: &GameType) -> Option<PathBuf> {
        self.state
            .game_configs
            .get(game_type)
            .map(|config| config.executable_path.clone())
    }

    fn scan_directory(&mut self, path: &Path, depth: usize) -> Vec<FileEntry> {
        let mut entries = Vec::new();
        
        if let Ok(read_dir) = fs::read_dir(path) {
            let mut dir_entries: Vec<_> = read_dir.flatten().collect();
            // Sort entries: directories first, then files, both alphabetically
            dir_entries.sort_by(|a, b| {
                let a_is_dir = a.path().is_dir();
                let b_is_dir = b.path().is_dir();
                
                if a_is_dir && !b_is_dir {
                    std::cmp::Ordering::Less
                } else if !a_is_dir && b_is_dir {
                    std::cmp::Ordering::Greater
                } else {
                    a.file_name().cmp(&b.file_name())
                }
            });

            for entry in dir_entries {
                let entry_path = entry.path();
                let is_directory = entry_path.is_dir();
                
                let mut file_entry = FileEntry::new(entry_path.clone(), is_directory);
                
                if is_directory && depth < 10 { // Limit recursion depth
                    file_entry.children = self.scan_directory(&entry_path, depth + 1);
                }
                
                entries.push(file_entry);
            }
        }
        
        entries
    }

    fn read_zip_contents(&self, zip_path: &Path) -> Result<Vec<ZipEntry>, Box<dyn std::error::Error>> {
        let file = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        
        let mut entries = Vec::new();
        
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let is_directory = file.name().ends_with('/');
            
            entries.push(ZipEntry {
                name: file.name().to_string(),
                is_directory,
            });
        }
        
        Ok(entries)
    }

    fn scan_assets_folder(&mut self, executable_path: &Path) {
        self.file_tree.clear();
        self.selected_file = None;

        // Get the directory containing the executable
        if let Some(parent_dir) = executable_path.parent() {
            let assets_dir = parent_dir.join("assets");
            
            println!("Scanning assets folder: {}", assets_dir.display());
            
            if assets_dir.exists() && assets_dir.is_dir() {
                self.file_tree = self.scan_directory(&assets_dir, 0);
                println!("Scanned file tree with {} root entries", self.file_tree.len());
            } else {
                println!("Assets folder not found: {}", assets_dir.display());
            }
        } else {
            println!("Could not get parent directory of executable: {}", executable_path.display());
        }
    }

    fn show_file_tree_ui(&mut self, ui: &mut egui::Ui) {
        let mut entries_to_process = std::mem::take(&mut self.file_tree);
        self.show_file_tree_internal(ui, &mut entries_to_process);
        self.file_tree = entries_to_process;
    }

    fn show_file_tree_internal(&mut self, ui: &mut egui::Ui, entries: &mut Vec<FileEntry>) {
        for entry in entries {
            let display_name = entry.path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();

            if entry.is_directory || entry.is_zip {
                // Handle ZIP files
                if entry.is_zip {
                    let initially_open = self.expanded_folders.contains(&entry.path);
                    
                    // Show ZIP icon and name in a horizontal layout for ALL games
                    ui.horizontal(|ui| {
                        if let Some(zip_icon) = self.file_icons.get("zip") {
                            egui::Image::new(zip_icon)
                                .max_size(egui::Vec2::splat(16.0))
                                .ui(ui);
                        }
                        
                        // Only show dropdown for Cars 2 games that support ZIP browsing
                        if let Some(game_type) = &self.state.selected_game {
                            if game_type.supports_zip_browsing() {
                                let response = egui::CollapsingHeader::new(&display_name)
                                    .default_open(initially_open)
                                    .show(ui, |ui| {
                                        // Load ZIP contents if not already loaded
                                        if !entry.zip_contents_loaded {
                                            match self.read_zip_contents(&entry.path) {
                                                Ok(zip_entries) => {
                                                    // Convert zip entries to file entries
                                                    for zip_entry in zip_entries {
                                                        let virtual_path = entry.path.join(&zip_entry.name);
                                                        let mut file_entry = FileEntry::new(virtual_path, zip_entry.is_directory);
                                                        file_entry.is_zip = false;
                                                        entry.children.push(file_entry);
                                                    }
                                                    entry.zip_contents_loaded = true;
                                                }
                                                Err(e) => {
                                                    ui.colored_label(egui::Color32::RED, 
                                                        format!("Failed to read ZIP: {}", e));
                                                }
                                            }
                                        }
                                        
                                        // Show ZIP contents
                                        self.show_file_tree_internal(ui, &mut entry.children);
                                    });

                                if response.header_response.clicked() {
                                    if self.expanded_folders.contains(&entry.path) {
                                        self.expanded_folders.remove(&entry.path);
                                    } else {
                                        self.expanded_folders.insert(entry.path.clone());
                                    }
                                }
                            } else {
                                // For non-Cars 2 games, just show the ZIP file as a regular file (non-expandable)
                                let is_selected = self.selected_file.as_ref() == Some(&entry.path);
                                if ui.selectable_label(is_selected, &display_name).clicked() {
                                    self.selected_file = Some(entry.path.clone());
                                }
                            }
                        }
                    });
                    continue;
                }

                // Regular directory (for all games)
                let initially_open = self.expanded_folders.contains(&entry.path);
                let response = egui::CollapsingHeader::new(&display_name)
                    .default_open(initially_open)
                    .show(ui, |ui| {
                        self.show_file_tree_internal(ui, &mut entry.children);
                    });

                // Update expanded state based on user interaction
                if response.header_response.clicked() {
                    if self.expanded_folders.contains(&entry.path) {
                        self.expanded_folders.remove(&entry.path);
                    } else {
                        self.expanded_folders.insert(entry.path.clone());
                    }
                }
            } else {
                // File - selectable with icon
                let is_selected = self.selected_file.as_ref() == Some(&entry.path);
                
                ui.horizontal(|ui| {
                    // Show icon if available
                    if let Some(icon) = self.get_file_icon(&entry.path) {
                        egui::Image::new(icon)
                            .max_size(egui::Vec2::splat(16.0))
                            .ui(ui);
                    } else {
                        // Placeholder for files without icons
                        ui.add_space(18.0);
                    }
                    
                    // Files inside ZIPs get green text (only for Cars 2 games that support ZIP browsing)
                    let is_in_zip = if let Some(game_type) = &self.state.selected_game {
                        game_type.supports_zip_browsing() && entry.path.components().any(|c| {
                            if let std::path::Component::Normal(name) = c {
                                if let Some(name_str) = name.to_str() {
                                    return name_str.to_lowercase().ends_with(".zip");
                                }
                            }
                            false
                        })
                    } else {
                        false
                    };
                    
                    if is_in_zip {
                        if ui.selectable_label(is_selected, egui::RichText::new(&display_name).color(egui::Color32::GREEN)).clicked() {
                            self.selected_file = Some(entry.path.clone());
                        }
                    } else {
                        if ui.selectable_label(is_selected, &display_name).clicked() {
                            self.selected_file = Some(entry.path.clone());
                        }
                    }
                });
            }
        }
    }

    fn show_game_selection(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tundra");
        ui.label("Select the game you want to edit:");

        for game_type in GameType::all() {
            let button_text = if let Some(path) = self.get_game_path(&game_type) {
                format!("{} (Configured: {})", game_type.as_str(), path.display())
            } else {
                game_type.as_str().to_string()
            };

            if ui.button(&button_text).clicked() {
                self.state.selected_game = Some(game_type.clone());
                
                if let Some(path) = self.get_game_path(&game_type) {
                    // If we already have a valid path, go directly to editor
                    if self.validate_executable(&game_type, &path) {
                        self.scan_assets_folder(&path);
                        self.state.current_step = AppStep::Editor;
                    } else {
                        // If path exists but is invalid, go to file selection
                        self.state.current_step = AppStep::FileSelection;
                    }
                } else {
                    // Otherwise, prompt for file selection
                    self.state.current_step = AppStep::FileSelection;
                }
                
                // Save state when game is selected
                self.save_state();
            }
            ui.add_space(10.0);
        }
    }

    fn show_file_selection(&mut self, ui: &mut egui::Ui) {
        // Clone the game type to avoid holding reference to self.state
        let game_type = match self.state.selected_game.clone() {
            Some(gt) => gt,
            None => {
                ui.heading("Tundra");
                ui.label("No game selected");
                if ui.button("Back to Game Selection").clicked() {
                    self.state.current_step = AppStep::GameSelection;
                }
                return;
            }
        };

        // Check if we already have a valid executable for this game
        if let Some(config) = self.state.game_configs.get(&game_type) {
            if self.validate_executable(&game_type, &config.executable_path) {
                // If we have a valid executable, automatically switch to editor
                let path = config.executable_path.clone();
                self.scan_assets_folder(&path);
                self.state.current_step = AppStep::Editor;
                return;
            }
        }

        ui.heading("Tundra");
        ui.label(format!("Select {} executable:", game_type.as_str()));
        ui.label(format!("Expected file: {}", game_type.expected_executable()));

        if ui.button("Browse for executable...").clicked() {
            self.open_file_dialog();
        }

        // Check if we have a config for this game type (even if invalid)
        if let Some(config) = self.state.game_configs.get(&game_type) {
            ui.add_space(10.0);
            ui.label(format!("Current selection: {}", config.executable_path.display()));
            
            if self.validate_executable(&game_type, &config.executable_path) {
                ui.colored_label(egui::Color32::GREEN, "Valid executable selected - opening editor...");
                // This should automatically trigger editor on next frame due to the check above
            } else {
                ui.colored_label(egui::Color32::YELLOW, "File selected but name doesn't match expected");
                ui.colored_label(egui::Color32::RED, "Please select the correct executable file");
            }
        } else {
            ui.add_space(10.0);
            ui.label("No executable selected yet.");
        }

        ui.add_space(10.0);
        if ui.button("Back to Game Selection").clicked() {
            self.state.current_step = AppStep::GameSelection;
        }
    }

    fn show_editor(&mut self, ctx: &egui::Context) {
        // Use SidePanel for the file list to ensure it takes full height
        egui::SidePanel::left("file_panel")
            .resizable(false)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.heading("File System");
                
                // Show current game info
                if let Some(game_type) = &self.state.selected_game {
                    if let Some(config) = self.state.game_configs.get(game_type) {
                        ui.label(format!("Game: {}", game_type.as_str()));
                        if let Some(parent_dir) = config.executable_path.parent() {
                            let assets_dir = parent_dir.join("assets");
                            ui.label(format!("Assets: {}", assets_dir.display()));
                        }
                    }
                }
                
                ui.separator();
                
                if self.file_tree.is_empty() {
                    ui.label("No files found in assets folder");
                    ui.label("Make sure there's an 'assets' folder next to the executable");
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            self.show_file_tree_ui(ui);
                        });
                }
            });

        // The rest of the space is for the main area
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(selected_path) = &self.selected_file {
                ui.heading("File Editor");
                ui.separator();
                
                let file_name = selected_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown");
                
                // Show file icon in editor header if available
                ui.horizontal(|ui| {
                    if let Some(icon) = self.get_file_icon(selected_path) {
                        egui::Image::new(icon)
                            .max_size(egui::Vec2::splat(24.0))
                            .ui(ui);
                    }
                    ui.label(format!("Selected file: {}", file_name));
                });
                
                ui.label(format!("Full path: {}", selected_path.display()));
                
                // Show file info
                if let Ok(metadata) = fs::metadata(selected_path) {
                    let file_size = metadata.len();
                    ui.label(format!("Size: {} bytes", file_size));
                    
                    // File extension info
                    if let Some(extension) = selected_path.extension().and_then(|e| e.to_str()) {
                        ui.label(format!("Type: {} file", extension.to_uppercase()));
                    }
                }
            } else {
                ui.heading("Tundra");
                ui.label("Select a file from the assets folder to begin editing");
            }
            
            // "Change Game" button in bottom right
            ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                if ui.button("Change Game").clicked() {
                    self.state.current_step = AppStep::GameSelection;
                    self.save_state();
                }
            });
        });
    }
}

impl eframe::App for TundraEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle file dialog on the main thread
        self.handle_file_dialog();

        match self.state.current_step {
            AppStep::GameSelection => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.show_game_selection(ui);
                });
            }
            AppStep::FileSelection => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.show_file_selection(ui);
                });
            }
            AppStep::Editor => {
                self.show_editor(ctx);
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Save to JSON file
        self.save_state();
        
        // Also save to eframe storage for compatibility
        if let Ok(serialized) = serde_json::to_string(&self.state) {
            storage.set_string("app_state", serialized);
        }
    }
}

fn main() -> eframe::Result<()> {
    // Load icon
    let icon = load_icon("src/art/icon.ico").expect("Failed to load app icon");
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Tundra")
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "Tundra",
        options,
        Box::new(|cc| Box::new(TundraEditor::new(cc))),
    )
}

fn load_icon(path: &str) -> Result<egui::IconData, image::ImageError> {
    let image = image::open(path)?;
    let image = image.into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    Ok(egui::IconData { rgba, width, height })
}