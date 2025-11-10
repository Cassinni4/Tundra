use eframe::egui;
use std::path::PathBuf;
use std::fs::File;
use super::binary_reader::BinaryReader;

#[derive(Debug, Clone)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub meshes: Vec<Mesh>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

pub struct ModelViewer {
    pub current_model: Option<Model>,
    pub camera_rotation: [f32; 2],
    pub camera_distance: f32,
    pub show_wireframe: bool,
    pub show_vertices: bool,
    pub vertex_scale: f32,
    pub debug_info: String,
}

impl Default for ModelViewer {
    fn default() -> Self {
        Self {
            current_model: None,
            camera_rotation: [0.0, 0.0],
            camera_distance: 5.0,
            show_wireframe: true,
            show_vertices: false,
            vertex_scale: 0.1,
            debug_info: String::new(),
        }
    }
}

impl ModelViewer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_model_from_files(&mut self, ibuf_path: &PathBuf, vbuf_path: &PathBuf) -> Result<(), String> {
        self.debug_info = format!("Loading model:\nIBUF: {}\nVBUF: {}", 
            ibuf_path.display(), vbuf_path.display());

        // Parse vertex buffer (VBUF)
        let vertices = match self.parse_vertex_buffer(vbuf_path) {
            Ok(v) => {
                self.debug_info.push_str(&format!("\nParsed {} vertices", v.len()));
                v
            }
            Err(e) => {
                self.debug_info.push_str(&format!("\nVBUF Error: {}", e));
                return Err(e);
            }
        };

        // Parse index buffer (IBUF)
        let indices = match self.parse_index_buffer(ibuf_path) {
            Ok(i) => {
                self.debug_info.push_str(&format!("\nParsed {} indices", i.len()));
                i
            }
            Err(e) => {
                self.debug_info.push_str(&format!("\nIBUF Error: {}", e));
                return Err(e);
            }
        };

        if vertices.is_empty() || indices.is_empty() {
            return Err("No vertices or indices found".to_string());
        }

        // Create mesh
        let mesh = Mesh {
            vertices,
            indices,
            name: "Disney Infinity Model".to_string(),
        };

        // Calculate bounding box
        let (bounds_min, bounds_max) = self.calculate_bounds(&[mesh.clone()]);

        self.current_model = Some(Model {
            meshes: vec![mesh],
            bounds_min,
            bounds_max,
        });

        self.debug_info.push_str(&format!("\nModel loaded successfully!"));
        Ok(())
    }

    fn parse_vertex_buffer(&self, vbuf_path: &PathBuf) -> Result<Vec<Vertex>, String> {
        let file = File::open(vbuf_path)
            .map_err(|e| format!("Failed to open VBUF file: {}", e))?;
        
        let mut reader = BinaryReader::new(file);
        
        // Try different vertex formats
        let file_size = std::fs::metadata(vbuf_path)
            .map(|m| m.len())
            .unwrap_or(0);
        
        let mut vertices = Vec::new();
        
        // Try simple position-only format first (12 bytes per vertex)
        let vertex_count = file_size / 12;
        if vertex_count > 0 && vertex_count < 100000 { // Sanity check
            if let Ok(simple_vertices) = self.parse_simple_vertices(&mut reader, vertex_count as usize) {
                vertices = simple_vertices;
            }
        }
        
        // If simple parsing failed, try more complex formats
        if vertices.is_empty() {
            // Reset and try alternative format
            let _ = reader.seek(0);
            if let Ok(complex_vertices) = self.parse_complex_vertices(&mut reader) {
                vertices = complex_vertices;
            }
        }
        
        if vertices.is_empty() {
            return Err("Could not parse any vertices from VBUF file".to_string());
        }
        
        Ok(vertices)
    }

    fn parse_simple_vertices(&self, reader: &mut BinaryReader<File>, count: usize) -> Result<Vec<Vertex>, String> {
        let mut vertices = Vec::with_capacity(count);
        
        for _ in 0..count {
            match reader.read_f32_array(3) {
                Ok(pos) => {
                    vertices.push(Vertex {
                        position: [pos[0], pos[1], pos[2]],
                        normal: [0.0, 1.0, 0.0], // Default normal
                        uv: [0.0, 0.0], // Default UV
                    });
                }
                Err(_) => break, // Stop if we can't read more
            }
        }
        
        Ok(vertices)
    }

    fn parse_complex_vertices(&self, reader: &mut BinaryReader<File>) -> Result<Vec<Vertex>, String> {
        let mut vertices = Vec::new();
        
        // Try to read until EOF
        while let Ok(pos) = reader.read_f32_array(3) {
            // Try to read normal (3 floats)
            let normal = reader.read_f32_array(3).unwrap_or_else(|_| vec![0.0, 1.0, 0.0]);
            
            // Try to read UV (2 floats)
            let uv = reader.read_f32_array(2).unwrap_or_else(|_| vec![0.0, 0.0]);
            
            vertices.push(Vertex {
                position: [pos[0], pos[1], pos[2]],
                normal: [normal[0], normal[1], normal[2]],
                uv: [uv[0], uv[1]],
            });
        }
        
        Ok(vertices)
    }

    fn parse_index_buffer(&self, ibuf_path: &PathBuf) -> Result<Vec<u16>, String> {
        let file = File::open(ibuf_path)
            .map_err(|e| format!("Failed to open IBUF file: {}", e))?;
        
        let mut reader = BinaryReader::new(file);
        let mut indices = Vec::new();
        
        // Read until EOF
        while let Ok(index) = reader.read_u16() {
            indices.push(index);
        }
        
        Ok(indices)
    }

    fn calculate_bounds(&self, meshes: &[Mesh]) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::MAX, f32::MAX, f32::MAX];
        let mut max = [f32::MIN, f32::MIN, f32::MIN];

        for mesh in meshes {
            for vertex in &mesh.vertices {
                for i in 0..3 {
                    if vertex.position[i] < min[i] {
                        min[i] = vertex.position[i];
                    }
                    if vertex.position[i] > max[i] {
                        max[i] = vertex.position[i];
                    }
                }
            }
        }

        // If no valid bounds found, use defaults
        if min[0] == f32::MAX {
            min = [-1.0, -1.0, -1.0];
            max = [1.0, 1.0, 1.0];
        }

        (min, max)
    }

    pub fn clear_model(&mut self) {
        self.current_model = None;
        self.debug_info.clear();
    }

    pub fn has_model(&self) -> bool {
        self.current_model.is_some()
    }

    pub fn show_ui(&mut self, ui: &mut egui::Ui, available_size: egui::Vec2) {
        ui.heading("Disney Infinity 3.0 Model Viewer");

        // Clone the model to avoid borrow issues
        let model_clone = self.current_model.clone();
        
        if let Some(model) = &model_clone {
            // Model info
            ui.label(format!("Meshes: {}", model.meshes.len()));
            ui.label(format!("Total vertices: {}", 
                model.meshes.iter().map(|m| m.vertices.len()).sum::<usize>()));
            ui.label(format!("Total indices: {}", 
                model.meshes.iter().map(|m| m.indices.len()).sum::<usize>()));
            ui.label(format!("Bounds: [{:.2}, {:.2}, {:.2}] to [{:.2}, {:.2}, {:.2}]", 
                model.bounds_min[0], model.bounds_min[1], model.bounds_min[2],
                model.bounds_max[0], model.bounds_max[1], model.bounds_max[2]));

            ui.separator();

            // Controls
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.show_wireframe, "Wireframe");
                ui.checkbox(&mut self.show_vertices, "Vertices");
                
                // Add a clear button
                if ui.button("Clear Model").clicked() {
                    self.clear_model();
                    return; // Return early to avoid using cleared model
                }
            });

            if self.show_vertices {
                ui.add(egui::Slider::new(&mut self.vertex_scale, 0.01..=1.0).text("Vertex Scale"));
            }

            // Debug info
            if ui.button("Show Debug Info").clicked() {
                // Debug info is already being collected during loading
            }
            if !self.debug_info.is_empty() {
                ui.label("Debug Info:");
                ui.text_edit_multiline(&mut self.debug_info);
            }

            ui.separator();

            // 3D View - pass the cloned model
            self.show_3d_view(ui, available_size, model);
        } else {
            ui.label("No model loaded. Select an IBUF/VBUF file pair to view.");
            ui.label("Note: Both .ibuf and .vbuf files must be selected.");
        }
    }

    fn show_3d_view(&mut self, ui: &mut egui::Ui, available_size: egui::Vec2, model: &Model) {
        let (response, painter) = ui.allocate_painter(available_size, egui::Sense::drag());

        // Draw a background so we can see the viewport area
        painter.rect_filled(
            response.rect,
            egui::Rounding::ZERO, // Fixed: use ZERO instead of none()
            egui::Color32::from_rgba_unmultiplied(20, 20, 40, 255),
        );

        // Handle camera rotation via dragging
        if response.dragged() {
            let delta = response.drag_delta();
            self.camera_rotation[0] += delta.x * 0.01;
            self.camera_rotation[1] += delta.y * 0.01;
            self.camera_rotation[1] = self.camera_rotation[1].clamp(-1.57, 1.57); // Clamp vertical rotation
        }

        // Handle zoom via scroll
        if response.hovered() {
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll_delta != 0.0 {
                self.camera_distance *= 1.0 - scroll_delta * 0.001;
                self.camera_distance = self.camera_distance.clamp(0.1, 50.0);
            }
        }

        // Calculate camera position
        let camera_pos = [
            self.camera_distance * self.camera_rotation[0].cos() * self.camera_rotation[1].cos(),
            self.camera_distance * self.camera_rotation[1].sin(),
            self.camera_distance * self.camera_rotation[0].sin() * self.camera_rotation[1].cos(),
        ];

        // Calculate model center and scale for view
        let center = [
            (model.bounds_min[0] + model.bounds_max[0]) * 0.5,
            (model.bounds_min[1] + model.bounds_max[1]) * 0.5,
            (model.bounds_min[2] + model.bounds_max[2]) * 0.5,
        ];

        let model_size = [
            model.bounds_max[0] - model.bounds_min[0],
            model.bounds_max[1] - model.bounds_min[1],
            model.bounds_max[2] - model.bounds_min[2],
        ];

        let max_size = model_size[0].max(model_size[1]).max(model_size[2]);
        let scale = if max_size > 0.0 { 2.0 / max_size } else { 1.0 };

        // Draw the model
        let mut triangle_count = 0;
        let mut vertex_count = 0;

        for mesh in &model.meshes {
            // Draw wireframe
            if self.show_wireframe && mesh.indices.len() >= 3 {
                for chunk in mesh.indices.chunks(3) {
                    if chunk.len() == 3 {
                        let idx0 = chunk[0] as usize;
                        let idx1 = chunk[1] as usize;
                        let idx2 = chunk[2] as usize;
                        
                        if idx0 < mesh.vertices.len() && idx1 < mesh.vertices.len() && idx2 < mesh.vertices.len() {
                            let v0 = &mesh.vertices[idx0];
                            let v1 = &mesh.vertices[idx1];
                            let v2 = &mesh.vertices[idx2];

                            let p0 = self.project_point(&v0.position, center, scale, &camera_pos, available_size);
                            let p1 = self.project_point(&v1.position, center, scale, &camera_pos, available_size);
                            let p2 = self.project_point(&v2.position, center, scale, &camera_pos, available_size);

                            // Only draw if points are within viewport
                            if self.is_point_in_viewport(p0, available_size) || 
                               self.is_point_in_viewport(p1, available_size) || 
                               self.is_point_in_viewport(p2, available_size) {
                                painter.line_segment([p0, p1], (2.0, egui::Color32::YELLOW));
                                painter.line_segment([p1, p2], (2.0, egui::Color32::YELLOW));
                                painter.line_segment([p2, p0], (2.0, egui::Color32::YELLOW));
                                triangle_count += 1;
                            }
                        }
                    }
                }
            }

            // Draw vertices
            if self.show_vertices {
                for vertex in &mesh.vertices {
                    let pos = self.project_point(&vertex.position, center, scale, &camera_pos, available_size);
                    if self.is_point_in_viewport(pos, available_size) {
                        painter.circle_filled(pos, self.vertex_scale * 4.0, egui::Color32::RED);
                        vertex_count += 1;
                    }
                }
            }
        }

        // Draw coordinate axes
        self.draw_coordinate_axes(&painter, center, scale, &camera_pos, available_size);

        // Draw stats in corner
        let stats_text = format!("Triangles: {} | Vertices: {}", triangle_count, vertex_count);
        painter.text(
            response.rect.left_bottom() + egui::Vec2::new(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            stats_text,
            egui::FontId::default(),
            egui::Color32::WHITE,
        );
    }

    fn project_point(&self, point: &[f32; 3], center: [f32; 3], scale: f32, camera_pos: &[f32; 3], viewport_size: egui::Vec2) -> egui::Pos2 {
        // Simple perspective projection
        let x = (point[0] - center[0]) * scale;
        let y = (point[1] - center[1]) * scale;
        let z = (point[2] - center[2]) * scale;

        // Simple camera transformation
        let screen_x = x - camera_pos[0];
        let screen_y = y - camera_pos[1];
        let screen_z = z - camera_pos[2];

        // Perspective divide
        let perspective = 1.0 / (screen_z + 5.0); // Add some offset to avoid division by zero

        let screen_x = screen_x * perspective * viewport_size.x * 0.5 + viewport_size.x * 0.5;
        let screen_y = screen_y * perspective * viewport_size.y * 0.5 + viewport_size.y * 0.5;

        egui::Pos2::new(screen_x, screen_y)
    }

    fn draw_coordinate_axes(&self, painter: &egui::Painter, center: [f32; 3], scale: f32, camera_pos: &[f32; 3], viewport_size: egui::Vec2) {
        let origin = self.project_point(&center, center, scale, camera_pos, viewport_size);
        
        let x_axis = [center[0] + 1.0, center[1], center[2]];
        let y_axis = [center[0], center[1] + 1.0, center[2]];
        let z_axis = [center[0], center[1], center[2] + 1.0];

        let x_end = self.project_point(&x_axis, center, scale, camera_pos, viewport_size);
        let y_end = self.project_point(&y_axis, center, scale, camera_pos, viewport_size);
        let z_end = self.project_point(&z_axis, center, scale, camera_pos, viewport_size);

        painter.line_segment([origin, x_end], (2.0, egui::Color32::RED));
        painter.line_segment([origin, y_end], (2.0, egui::Color32::GREEN));
        painter.line_segment([origin, z_end], (2.0, egui::Color32::BLUE));

        painter.text(x_end, egui::Align2::LEFT_TOP, "X", egui::FontId::default(), egui::Color32::RED);
        painter.text(y_end, egui::Align2::LEFT_TOP, "Y", egui::FontId::default(), egui::Color32::GREEN);
        painter.text(z_end, egui::Align2::LEFT_TOP, "Z", egui::FontId::default(), egui::Color32::BLUE);
    }

    fn is_point_in_viewport(&self, point: egui::Pos2, viewport_size: egui::Vec2) -> bool {
        point.x >= 0.0 && point.x <= viewport_size.x && point.y >= 0.0 && point.y <= viewport_size.y
    }
}