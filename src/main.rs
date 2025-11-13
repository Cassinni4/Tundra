use eframe::egui;
use eframe::egui::Widget;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::sync::{Arc, Mutex};
use std::thread;

mod in3;
use in3::ViewModel;
use in3::read_zip::DisneyInfinityZipReader;

mod gen;
use gen::MtbViewer;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
enum GameType {
    DisneyInfinity30,
    Cars2TheVideoGame,
    Cars2Arcade,
    Cars3DrivenToWinXB1,
    ToyShit3,
}

impl GameType {
    fn as_str(&self) -> &'static str {
        match self {
            GameType::DisneyInfinity30 => "Disney Infinity 3.0",
            GameType::Cars2TheVideoGame => "Cars 2: The Video Game",
            GameType::Cars2Arcade => "Cars 2 Arcade",
            GameType::Cars3DrivenToWinXB1 => "Cars 3: Driven To Win (Xbox One)",
            GameType::ToyShit3 => "Toy Story 3",
        }
    }

    fn expected_executable(&self) -> &'static str {
        match self {
            GameType::DisneyInfinity30 => "DisneyInfinity3.exe",
            GameType::Cars2TheVideoGame => "Game-Cars.exe",
            GameType::Cars2Arcade => "sdaemon.exe",
            GameType::Cars3DrivenToWinXB1 => "game.consumer.exe",
            GameType::ToyShit3 => "Game-TS3.exe",
        }
    }

    fn all() -> Vec<Self> {
        vec![
            GameType::DisneyInfinity30,
            GameType::Cars2TheVideoGame,
            GameType::Cars2Arcade,
            GameType::Cars3DrivenToWinXB1,
            GameType::ToyShit3,
        ]
    }

    fn supports_zip_browsing(&self) -> bool {
        matches!(self, GameType::Cars2TheVideoGame | GameType::Cars2Arcade | GameType::DisneyInfinity30 | GameType::ToyShit3)
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
    theme: Theme,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum AppStep {
    GameSelection,
    FileSelection,
    Editor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum Theme {
    Dark,
    Light,
    System,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            selected_game: None,
            game_configs: HashMap::new(),
            current_step: AppStep::GameSelection,
            theme: Theme::Dark,
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
    model_viewer: ViewModel::ModelViewer,
    show_options: bool,
    scan_progress: Option<ScanProgress>,
    scan_thread: Option<thread::JoinHandle<Vec<FileEntry>>>,
    scan_cancel: Arc<Mutex<bool>>,
    mtb_viewer: MtbViewer,
    egui_ctx: Option<egui::Context>,
    should_exit: bool,
    show_crash_dialog: bool,
    temp_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct ScanProgress {
    current_path: PathBuf,
    total_files: usize,
    processed_files: usize,
    start_time: Instant,
}

impl TundraEditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config_path = PathBuf::from("tundra_config.json");
        
        // Create temp directory for ZIP extraction
        let temp_dir = PathBuf::from("temp");
        if let Err(e) = fs::create_dir_all(&temp_dir) {
            eprintln!("Failed to create temp directory: {}", e);
        }
        
        let mut app = Self {
            state: AppState::default(),
            pending_file_selection: false,
            selected_file: None,
            file_tree: Vec::new(),
            expanded_folders: std::collections::HashSet::new(),
            file_icons: HashMap::new(),
            config_path,
            model_viewer: ViewModel::ModelViewer::new(),
            show_options: false,
            scan_progress: None,
            scan_thread: None,
            scan_cancel: Arc::new(Mutex::new(false)),
            mtb_viewer: MtbViewer::new(),
            egui_ctx: Some(cc.egui_ctx.clone()),
            should_exit: false,
            show_crash_dialog: false,
            temp_dir,
        };

        // Load file icons
        app.load_file_icons(cc);

        // Try to load state from JSON file
        app.load_from_json();

        // Apply theme
        app.apply_theme(cc);

        app
    }

    fn apply_theme(&self, cc: &eframe::CreationContext<'_>) {
        match self.state.theme {
            Theme::Dark => {
                cc.egui_ctx.set_visuals(egui::Visuals::dark());
            }   
            Theme::Light => {
                cc.egui_ctx.set_visuals(egui::Visuals::light());
            }
            Theme::System => {
                // System theme follows the OS preference
                #[cfg(target_os = "windows")]
                {
                    use winreg::enums::*;
                    use winreg::RegKey;
                
                    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                    if let Ok(personalize) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize") {
                        if let Ok(apps_use_light_theme) = personalize.get_value::<u32, _>("AppsUseLightTheme") {
                            if apps_use_light_theme == 1 {
                                cc.egui_ctx.set_visuals(egui::Visuals::light());
                                return;
                            }
                        }
                    }
                }
            
                #[cfg(target_os = "macos")]
                {
                    use std::process::Command;
                
                    if let Ok(output) = Command::new("defaults").args(&["read", "-g", "AppleInterfaceStyle"]).output() {
                        if output.status.success() {
                            let theme = String::from_utf8_lossy(&output.stdout);
                            if theme.to_lowercase().contains("dark") {
                                cc.egui_ctx.set_visuals(egui::Visuals::dark());
                                return;
                            }
                        }
                    }
                    cc.egui_ctx.set_visuals(egui::Visuals::light());
                    return;
                }
            
                #[cfg(target_os = "linux")]
                {
                    use std::process::Command;
                
                    // Try to detect GTK theme
                    if let Ok(output) = Command::new("gsettings").args(&["get", "org.gnome.desktop.interface", "gtk-theme"]).output() {
                        if output.status.success() {
                            let theme = String::from_utf8_lossy(&output.stdout).to_lowercase();
                            if theme.contains("dark") {
                                cc.egui_ctx.set_visuals(egui::Visuals::dark());
                                return;
                            }
                        }
                    }
                }
            
                // Default fallback to dark theme
                cc.egui_ctx.set_visuals(egui::Visuals::dark());
            }
        }
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
                            if game_type != &GameType::Cars3DrivenToWinXB1 {
                                if self.validate_executable(game_type, &config.executable_path) {
                                    let path = config.executable_path.clone();
                                    self.scan_assets_folder(&path);
                                }
                            } else {
                                if self.validate_executable(game_type, &config.executable_path) {
                                    let path = config.executable_path.clone();
                                    self.scan_dtw_folder(&path);
                                }
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
            ("bik", "src/art/bik.png"),
            ("dnax", "src/art/lua.png"),
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

    fn handle_file_dialog(&mut self, _ctx: &egui::Context) {
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
                        if game_type != GameType::Cars3DrivenToWinXB1 {
                            self.scan_assets_folder(&file_path);
                        } else {
                            self.scan_dtw_folder(&file_path);
                        }
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

    fn scan_directory_threaded(path: PathBuf, cancel_flag: Arc<Mutex<bool>>) -> Vec<FileEntry> {
        let mut entries = Vec::new();
        
        // Check if cancelled before starting
        if *cancel_flag.lock().unwrap() {
            return entries;
        }
        
        if let Ok(read_dir) = fs::read_dir(&path) {
            let mut dir_entries: Vec<_> = read_dir.flatten().collect();
            
            // Sort entries: directories first, then files
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
                // Check cancellation flag periodically
                if *cancel_flag.lock().unwrap() {
                    break;
                }
                
                let entry_path = entry.path();
                let file_name = entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();

                // Cars 3 specific ignore list
                let ignore = [
                    "appdata.bin",
                    "appxmanifest.xml",
                    "buildstamp.lua",
                    "Catalog000.bin",
                    "game.consumer.exe",
                    "microsoft.xbox.gamechat.dll",
                    "microsoft.xbox.gamechat.winmd",
                    "microsoft.xbox.services.dll",
                    "microsoft.xbox.services.winmd",
                    "resources.pri",
                    "subheaps.xml",
                    "threadmonitor.dll",
                    "update",
                    "Update.AlignmentChunk"
                ];

                if ignore.contains(&file_name) {
                    continue;
                }

                let is_directory = entry_path.is_dir();
                
                let mut file_entry = FileEntry::new(entry_path.clone(), is_directory);
                
                // Recursively scan directories (with cancellation check)
                if is_directory {
                    file_entry.children = Self::scan_directory_threaded(entry_path, cancel_flag.clone());
                }
                
                entries.push(file_entry);
            }
        }
        
        entries
    }

    fn read_zip_contents(&self, zip_path: &Path) -> Result<Vec<ZipEntry>, Box<dyn std::error::Error>> {
        // Check if this is a Disney Infinity 3.0 encrypted zip
        if let Some(game_type) = &self.state.selected_game {
            if matches!(game_type, GameType::DisneyInfinity30) {
                println!("Attempting to read as Disney Infinity zip: {}", zip_path.display());
                
                // First check if it's actually a Disney Infinity zip
                if DisneyInfinityZipReader::is_disney_infinity_zip(zip_path) {
                    println!("Detected as Disney Infinity encrypted zip");
                    match DisneyInfinityZipReader::read_zip_contents(zip_path) {
                        Ok(di_entries) => {
                            println!("Successfully decrypted {} entries", di_entries.len());
                            // Convert DisneyInfinityZipEntry to our local ZipEntry
                            let entries: Vec<ZipEntry> = di_entries
                                .into_iter()
                                .map(|di_entry| ZipEntry {
                                    name: di_entry.name,
                                    is_directory: di_entry.is_directory,
                                })
                                .collect();
                            return Ok(entries);
                        }
                        Err(e) => {
                            println!("Disney Infinity zip decryption failed: {}", e);
                            // Fall through to regular zip reading
                        }
                    }
                } else {
                    println!("Not a Disney Infinity encrypted zip, trying regular zip");
                }
            }
        }
        
        // Regular zip reading
        println!("Reading as regular zip: {}", zip_path.display());
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
        
        println!("Found {} entries in regular zip", entries.len());
        Ok(entries)
    }

    fn extract_zip_file(&self, zip_path: &Path, entry_name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if let Some(game_type) = &self.state.selected_game {
            if matches!(game_type, GameType::DisneyInfinity30) {
                // Try to find the entry in the DI3 zip
                let entries = DisneyInfinityZipReader::read_zip_contents(zip_path)?;
                if let Some(entry) = entries.iter().find(|e| e.name == entry_name) {
                    return DisneyInfinityZipReader::extract_file(zip_path, entry);
                }
            }
        }
        
        // Fall back to regular zip extraction
        let file = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut file = archive.by_name(entry_name)?;
        
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        
        Ok(contents)
    }

    fn extract_zip_to_temp(&self, zip_path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Create a unique temp directory for this zip file
        let zip_file_name = zip_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown_zip");
        
        let extract_dir = self.temp_dir.join(zip_file_name);
        
        // Clear existing directory if it exists
        if extract_dir.exists() {
            fs::remove_dir_all(&extract_dir)?;
        }
        
        // Create the directory
        fs::create_dir_all(&extract_dir)?;
        
        println!("Extracting {} to {}", zip_path.display(), extract_dir.display());
        
        // Extract based on game type
        if let Some(game_type) = &self.state.selected_game {
            if matches!(game_type, GameType::DisneyInfinity30) && DisneyInfinityZipReader::is_disney_infinity_zip(zip_path) {
                // Use Disney Infinity extraction
                let entries = DisneyInfinityZipReader::read_zip_contents(zip_path)?;
                
                for entry in entries {
                    if !entry.is_directory {
                        match DisneyInfinityZipReader::extract_file(zip_path, &entry) {
                            Ok(content) => {
                                let file_path = extract_dir.join(&entry.name);
                                
                                // Create parent directories if needed
                                if let Some(parent) = file_path.parent() {
                                    fs::create_dir_all(parent)?;
                                }
                                
                                fs::write(&file_path, content)?;
                                println!("Extracted: {}", entry.name);
                            }
                            Err(e) => {
                                eprintln!("Failed to extract {}: {}", entry.name, e);
                            }
                        }
                    }
                }
            } else {
                // Use regular zip extraction
                let file = fs::File::open(zip_path)?;
                let mut archive = zip::ZipArchive::new(file)?;
                
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i)?;
                    let file_name = file.name().to_string();
                    
                    // Skip directories (they're created automatically)
                    if file_name.ends_with('/') {
                        continue;
                    }
                    
                    let file_path = extract_dir.join(&file_name);
                    
                    // Create parent directories if needed
                    if let Some(parent) = file_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    
                    let mut content = Vec::new();
                    file.read_to_end(&mut content)?;
                    
                    fs::write(&file_path, content)?;
                    println!("Extracted: {}", file_name);
                }
            }
        }
        
        println!("Extraction complete: {} files extracted", extract_dir.display());
        Ok(extract_dir)
    }

    fn scan_assets_folder(&mut self, executable_path: &Path) {
        // Cancel any ongoing scan
        *self.scan_cancel.lock().unwrap() = true;
        if let Some(thread) = self.scan_thread.take() {
            let _ = thread.join();
        }
        
        // Reset cancel flag
        *self.scan_cancel.lock().unwrap() = false;
        
        self.file_tree.clear();
        self.selected_file = None;
        self.model_viewer.clear_model();
        self.mtb_viewer.clear();

        // Get the directory containing the executable
        if let Some(parent_dir) = executable_path.parent() {
            let assets_dir = parent_dir.join("assets");
            
            println!("Starting threaded scan of: {}", assets_dir.display());
            
            if assets_dir.exists() && assets_dir.is_dir() {
                let scan_path = assets_dir.clone(); // Clone here to avoid move
                let cancel_flag = self.scan_cancel.clone();
                
                // Start threaded scan
                self.scan_thread = Some(thread::spawn(move || {
                    Self::scan_directory_threaded(scan_path, cancel_flag)
                }));
                
                // Show progress immediately
                self.scan_progress = Some(ScanProgress {
                    current_path: assets_dir,
                    total_files: 0, // We don't know the total yet
                    processed_files: 0,
                    start_time: Instant::now(),
                });
            } else {
                println!("Assets folder not found: {}", assets_dir.display());
                // Fall back to scanning the parent directory
                let scan_path = parent_dir.to_path_buf();
                let cancel_flag = self.scan_cancel.clone();
                
                self.scan_thread = Some(thread::spawn(move || {
                    Self::scan_directory_threaded(scan_path, cancel_flag)
                }));
                
                self.scan_progress = Some(ScanProgress {
                    current_path: parent_dir.to_path_buf(),
                    total_files: 0,
                    processed_files: 0,
                    start_time: Instant::now(),
                });
            }
        } else {
            println!("Could not get parent directory of executable: {}", executable_path.display());
        }
    }

    fn scan_dtw_folder(&mut self, executable_path: &Path) {
        // Cancel any ongoing scan
        *self.scan_cancel.lock().unwrap() = true;
        if let Some(thread) = self.scan_thread.take() {
            let _ = thread.join();
        }
        
        // Reset cancel flag
        *self.scan_cancel.lock().unwrap() = false;
        
        self.file_tree.clear();
        self.selected_file = None;
        self.model_viewer.clear_model();
        self.mtb_viewer.clear();

        // Get the directory containing the executable
        if let Some(parent_dir) = executable_path.parent() {
            println!("Starting threaded scan of: {}", parent_dir.display());
            
            let scan_path = parent_dir.to_path_buf();
            let cancel_flag = self.scan_cancel.clone();
            
            self.scan_thread = Some(thread::spawn(move || {
                Self::scan_directory_threaded(scan_path, cancel_flag)
            }));
            
            self.scan_progress = Some(ScanProgress {
                current_path: parent_dir.to_path_buf(),
                total_files: 0,
                processed_files: 0,
                start_time: Instant::now(),
            });
        } else {
            println!("Could not get parent directory of executable: {}", executable_path.display());
        }
    }

    fn check_scan_completion(&mut self) {
        if let Some(thread) = &self.scan_thread {
            if thread.is_finished() {
                if let Some(thread) = self.scan_thread.take() {
                    match thread.join() {
                        Ok(result) => {
                            self.file_tree = result;
                            self.scan_progress = None;
                            println!("Scan completed with {} root entries", self.file_tree.len());
                            
                            // Log total file count
                            let total_files = self.count_files(&self.file_tree);
                            println!("Total files and directories found: {}", total_files);
                        }
                        Err(e) => {
                            eprintln!("Scan thread panicked: {:?}", e);
                            self.scan_progress = None;
                        }
                    }
                }
            }
        }
    }

    fn count_files(&self, entries: &[FileEntry]) -> usize {
        let mut count = entries.len();
        for entry in entries {
            if entry.is_directory {
                count += self.count_files(&entry.children);
            }
        }
        count
    }

    fn handle_model_file_selection(&mut self, file_path: &PathBuf, ctx: &egui::Context) {
        println!("File selected: {}", file_path.display());
        
        if let Some(extension) = file_path.extension().and_then(|e| e.to_str()) {
            // Handle model files
            if extension.eq_ignore_ascii_case("ibuf") || extension.eq_ignore_ascii_case("vbuf") {
                // Find the corresponding file
                let base_name = file_path.with_extension("");
                let other_extension = if extension.eq_ignore_ascii_case("ibuf") { "vbuf" } else { "ibuf" };
                let other_file = base_name.with_extension(other_extension);
                
                println!("Looking for corresponding file: {}", other_file.display());
                
                if other_file.exists() {
                    let (ibuf_path, vbuf_path) = if extension.eq_ignore_ascii_case("ibuf") {
                        (file_path.clone(), other_file)
                    } else {
                        (other_file, file_path.clone())
                    };
                    
                    println!("Loading model from:\n  IBUF: {}\n  VBUF: {}", 
                        ibuf_path.display(), vbuf_path.display());
                    
                    match self.model_viewer.load_model_from_files(&ibuf_path, &vbuf_path) {
                        Ok(_) => {
                            println!("Successfully loaded model from {} and {}", 
                                ibuf_path.display(), vbuf_path.display());
                        }
                        Err(e) => {
                            eprintln!("Failed to load model: {}", e);
                        }
                    }
                } else {
                    println!("Corresponding {} file not found: {}", other_extension, other_file.display());
                    self.model_viewer.clear_model();
                }
                return;
            }
            
            // Handle MTB and TBODY files for Disney Infinity 3.0
            if let Some(game_type) = &self.state.selected_game {
                if matches!(game_type, GameType::DisneyInfinity30) {
                    if extension.eq_ignore_ascii_case("mtb") {
                        println!("Loading MTB file: {}", file_path.display());
                        if let Err(e) = self.mtb_viewer.load_mtb_file(file_path, ctx) {
                            eprintln!("Failed to load MTB file: {}", e);
                        }
                        return;
                    } else if extension.eq_ignore_ascii_case("tbody") {
                        println!("Loading TBODY file: {}", file_path.display());
                        if let Err(e) = self.mtb_viewer.load_tbody_file(file_path, ctx) {
                            eprintln!("Failed to load TBODY file: {}", e);
                        }
                        return;
                    }
                }
            }
        }
        
        // Clear both viewers if it's not a supported file type
        self.model_viewer.clear_model();
        self.mtb_viewer.clear();
    }

    fn show_file_tree_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Check if scan is complete
        self.check_scan_completion();

        // Show progress if scanning
        if let Some(progress) = &self.scan_progress {
            ui.heading("Scanning Files...");
            ui.label(format!("Scanning: {}", progress.current_path.display()));
            ui.label(format!("Elapsed: {:?}", progress.start_time.elapsed()));
            ui.add(egui::Spinner::new().size(32.0));
            ui.label("This may take a while for large directories...");
            return;
        }

        if self.file_tree.is_empty() {
            ui.label("No files found");
            return;
        }

        let mut entries_to_process = std::mem::take(&mut self.file_tree);
        self.show_file_tree_internal(ui, &mut entries_to_process, ctx);
        self.file_tree = entries_to_process;
    }

    fn show_file_tree_internal(&mut self, ui: &mut egui::Ui, entries: &mut Vec<FileEntry>, ctx: &egui::Context) {
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
                    
                        // Only show dropdown for games that support ZIP browsing
                        if let Some(game_type) = &self.state.selected_game {
                            if game_type.supports_zip_browsing() {
                                let response = egui::CollapsingHeader::new(&display_name)
                                    .default_open(initially_open)
                                    .show(ui, |ui| {
                                        // Load ZIP contents if not already loaded
                                        if !entry.zip_contents_loaded {
                                            // Extract ZIP to temp directory and scan it
                                            match self.extract_zip_to_temp(&entry.path) {
                                                Ok(extract_dir) => {
                                                    // Scan the extracted directory
                                                    let cancel_flag = Arc::new(Mutex::new(false));
                                                    let extracted_entries = Self::scan_directory_threaded(extract_dir, cancel_flag);
                                                    
                                                    // Add extracted entries as children
                                                    for mut extracted_entry in extracted_entries {
                                                        // Mark these as extracted files (not ZIPs)
                                                        extracted_entry.is_zip = false;
                                                        entry.children.push(extracted_entry);
                                                    }
                                                    
                                                    entry.zip_contents_loaded = true;
                                                    println!("ZIP contents loaded and extracted to temp directory");
                                                }
                                                Err(e) => {
                                                    ui.colored_label(egui::Color32::RED, 
                                                        format!("Failed to extract ZIP: {}", e));
                                                }
                                            }
                                        }
                                        
                                        // Show ZIP contents
                                        self.show_file_tree_internal(ui, &mut entry.children, ctx);
                                    });

                                if response.header_response.clicked() {
                                    if self.expanded_folders.contains(&entry.path) {
                                        self.expanded_folders.remove(&entry.path);
                                    } else {
                                        self.expanded_folders.insert(entry.path.clone());
                                    }
                                }
                            } else {
                                // For games that don't support ZIP browsing, just show the ZIP file as a regular file (non-expandable)
                                let is_selected = self.selected_file.as_ref() == Some(&entry.path);
                                if ui.selectable_label(is_selected, &display_name).clicked() {
                                    self.selected_file = Some(entry.path.clone());
                                    self.handle_model_file_selection(&entry.path, ctx);
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
                        self.show_file_tree_internal(ui, &mut entry.children, ctx);
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
                
                    // Check if this file is from a ZIP extraction (in temp directory)
                    let is_extracted_from_zip = entry.path.starts_with(&self.temp_dir);
                
                    // Files inside ZIPs or extracted from ZIPs get green text (only for games that support ZIP browsing)
                    let should_be_green = if let Some(game_type) = &self.state.selected_game {
                        game_type.supports_zip_browsing() && 
                        (entry.path.components().any(|c| {
                            if let std::path::Component::Normal(name) = c {
                                if let Some(name_str) = name.to_str() {
                                    return name_str.to_lowercase().ends_with(".zip");
                                }
                            }
                            false
                        }) || is_extracted_from_zip)
                    } else {
                        false
                    };
                
                    if should_be_green {
                        if ui.selectable_label(is_selected, egui::RichText::new(&display_name).color(egui::Color32::GREEN)).clicked() {
                            self.selected_file = Some(entry.path.clone());
                            self.handle_model_file_selection(&entry.path, ctx);
                        }
                    } else {
                        if ui.selectable_label(is_selected, &display_name).clicked() {
                            self.selected_file = Some(entry.path.clone());
                            self.handle_model_file_selection(&entry.path, ctx);
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
                        if game_type != GameType::Cars3DrivenToWinXB1 {
                            self.scan_assets_folder(&path);
                        } else {
                            self.scan_dtw_folder(&path);
                        }
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

    fn show_file_selection(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
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
                if game_type != GameType::Cars3DrivenToWinXB1 {
                    self.scan_assets_folder(&path);
                } else {
                    self.scan_dtw_folder(&path);
                }
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

    fn run_game(&self) {
        if let Some(game_type) = &self.state.selected_game {
            if let Some(config) = self.state.game_configs.get(game_type) {
                let executable_path = &config.executable_path;
                
                println!("Attempting to run game: {}", executable_path.display());
                
                match std::process::Command::new(executable_path).spawn() {
                    Ok(_) => {
                        println!("Successfully launched game: {}", game_type.as_str());
                    }
                    Err(e) => {
                        eprintln!("Failed to launch game: {}", e);
                    }
                }
            } else {
                eprintln!("No executable configured for game: {}", game_type.as_str());
            }
        } else {
            eprintln!("No game selected");
        }
    }

    fn show_options_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Options");
        ui.separator();
        
        ui.label("Theme:");
        ui.horizontal(|ui| {
            let previous_theme = self.state.theme.clone();
            
            ui.radio_value(&mut self.state.theme, Theme::Dark, "Dark");
            ui.radio_value(&mut self.state.theme, Theme::Light, "Light");
            ui.radio_value(&mut self.state.theme, Theme::System, "System");
            
            // Apply theme immediately if changed
            if self.state.theme != previous_theme {
                match self.state.theme {
                    Theme::Dark => {
                        ctx.set_visuals(egui::Visuals::dark());
                    }
                    Theme::Light => {
                        ctx.set_visuals(egui::Visuals::light());
                    }
                    Theme::System => {
                        // For System theme, we'd need to re-detect the system preference
                        // For now, we'll just use dark as fallback
                        ctx.set_visuals(egui::Visuals::dark());
                    }
                }
                self.save_state();
            }
        });
        
        ui.separator();
        if ui.button("Close").clicked() {
            self.show_options = false;
        }
    }

    fn show_crash_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog_open = self.show_crash_dialog;
        
        egui::Window::new("ruh-oh!")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .title_bar(false)
            .fixed_size(egui::Vec2::new(400.0, 200.0))
            .open(&mut dialog_open)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    
                    ui.heading("why you modding this game");
                    ui.add_space(10.0);
                    
                    ui.label("ts3 is buns");
                    ui.label("ts3 modding will never exist");
                    ui.add_space(20.0);
                    
                    ui.label("bye");
                    ui.add_space(20.0);
                    
                    if ui.button("Okay").clicked() {
                        self.should_exit = true;
                        self.show_crash_dialog = false;
                    }
                });
            });
            
        self.show_crash_dialog = dialog_open;
    }

    fn show_regular_file_info(&mut self, ui: &mut egui::Ui) {
        if let Some(selected_path) = &self.selected_file {
            ui.heading("File Editor");
            ui.separator();
            
            let file_name = selected_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            
            ui.horizontal(|ui| {
                if let Some(icon) = self.get_file_icon(selected_path) {
                    egui::Image::new(icon)
                        .max_size(egui::Vec2::splat(24.0))
                        .ui(ui);
                }
                ui.label(format!("Selected file: {}", file_name));
            });
            
            ui.label(format!("Full path: {}", selected_path.display()));
            
            if let Ok(metadata) = fs::metadata(selected_path) {
                let file_size = metadata.len();
                ui.label(format!("Size: {} bytes", file_size));
                
                if let Some(extension) = selected_path.extension().and_then(|e| e.to_str()) {
                    ui.label(format!("Type: {} file", extension.to_uppercase()));
                }
            }
        } else {
            ui.heading("Tundra");
            ui.label("Select a file from the assets folder to begin editing");
        }
    }

    fn show_editor(&mut self, ctx: &egui::Context) {
        // Check scan completion
        self.check_scan_completion();

        // why you playin this fuckass game
        if let Some(game_type) = &self.state.selected_game {
            if matches!(game_type, GameType::ToyShit3) && !self.show_crash_dialog && !self.should_exit {
                let crash_chance = 0.00000005; // 0.000005%
                if rand::random::<f64>() < crash_chance {
                    println!("Why the fuck are you modding this game? Remember, Toy Story 3 modding doesn't exist.");
                    self.show_crash_dialog = true;
                }
            }
        }

        // If crash dialog is showing, show it and block the rest of the UI
        if self.show_crash_dialog {
            self.show_crash_dialog(ctx);
            return; // Block the rest of the UI
        }

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
                            if game_type != &GameType::Cars3DrivenToWinXB1 {
                                let assets_dir = parent_dir.join("assets");
                                ui.label(format!("Assets: {}", assets_dir.display()));
                            } else {
                                ui.label(format!("Directory: {}", parent_dir.display()));
                            }
                        }
                    }
                }
                
                // Show file count if scan is complete
                if self.scan_progress.is_none() && !self.file_tree.is_empty() {
                    let total_files = self.count_files(&self.file_tree);
                    ui.label(format!("Total files: {}", total_files));
                }
                
                ui.separator();
                
                if self.file_tree.is_empty() && self.scan_progress.is_none() {
                    ui.label("No files found");
                    if let Some(game_type) = &self.state.selected_game {
                        if game_type != &GameType::Cars3DrivenToWinXB1 {
                            ui.label("Make sure there's an 'assets' folder next to the executable");
                        }
                    }
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            self.show_file_tree_ui(ui, ctx);
                        });
                }
            });

        // Show options window if needed
        if self.show_options {
            egui::Window::new("Options")
                .collapsible(false)
                .resizable(true)
                .default_width(400.0)
                .show(ctx, |ui| {
                    self.show_options_menu(ui, ctx);
                });
        }

        // The rest of the space is for the main area
        egui::CentralPanel::default().show(ctx, |ui| {
            // Check if we're viewing a Disney Infinity model or textures
            if let Some(game_type) = &self.state.selected_game {
                if matches!(game_type, GameType::DisneyInfinity30) {
                    // Check what type of content we should show
                    if self.model_viewer.has_model() {
                        // Show model viewer
                        let available_size = ui.available_size();
                        self.model_viewer.show_ui(ui, available_size);
                    } else if self.mtb_viewer.has_content() {
                        // Show MTB/TBODY viewer
                        let available_size = ui.available_size();
                        self.mtb_viewer.show_ui(ui, available_size, ctx);
                    } else {
                        // Show regular file info
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.show_regular_file_info(ui);
                        });
                    }
                } else {
                    // For other games, show regular file info
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.show_regular_file_info(ui);
                    });
                }
            } else {
                // No game selected, show regular file info
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_regular_file_info(ui);
                });
            }
            
            // "Run Game", "Options", and "Change Game" buttons in bottom right - show them OVER the model viewer
            ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                if ui.button("Change Game").clicked() {
                    self.state.current_step = AppStep::GameSelection;
                    self.save_state();
                }
                
                if ui.button("Options").clicked() {
                    self.show_options = true;
                }
                
                if ui.button("Run Game").clicked() {
                    self.run_game();
                }
            });
        });
    }
}

impl eframe::App for TundraEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle file dialog on the main thread
        self.handle_file_dialog(ctx);

        // Check if we should exit the application
        if self.should_exit {
            println!("TS3 modding will never exist");
            std::process::exit(0);
        }

        match self.state.current_step {
            AppStep::GameSelection => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.show_game_selection(ui);
                });
            }
            AppStep::FileSelection => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.show_file_selection(ui, ctx);
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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        println!("Tundra editor is shutting down");
        
        // Clean up temp directory
        if let Err(e) = fs::remove_dir_all(&self.temp_dir) {
            eprintln!("Failed to clean up temp directory: {}", e);
        } else {
            println!("Cleaned up temp directory: {}", self.temp_dir.display());
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