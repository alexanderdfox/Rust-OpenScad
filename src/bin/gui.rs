//! OpenSCAD-like GUI: code editor, 3D preview, console, render/export.
//! Polished dark theme, grid/axes, better shading, auto-preview, examples.

use eframe::egui;
use manifold_rust::manifold::Manifold;
use openscad_rs::stl;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("openscad_rs"),
        ..Default::default()
    };
    eframe::run_native(
        "openscad_rs",
        options,
        Box::new(|cc| {
            // Prefer a clean dark theme close to classic OpenSCAD
            let mut visuals = egui::Visuals::dark();
            visuals.window_fill = egui::Color32::from_rgb(28, 30, 36);
            visuals.panel_fill = egui::Color32::from_rgb(32, 34, 40);
            visuals.extreme_bg_color = egui::Color32::from_rgb(22, 24, 28);
            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(40, 42, 50);
            visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(48, 50, 60);
            visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 64, 78);
            visuals.widgets.active.bg_fill = egui::Color32::from_rgb(70, 110, 160);
            visuals.selection.bg_fill = egui::Color32::from_rgb(55, 95, 150);
            visuals.override_text_color = Some(egui::Color32::from_rgb(220, 222, 228));
            cc.egui_ctx.set_visuals(visuals);

            // Slightly larger default fonts for readability
            let mut style = (*cc.egui_ctx.style()).clone();
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                egui::FontId::monospace(13.5),
            );
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(App::new()))
        }),
    )
}

struct MeshPreview {
    positions: Vec<[f32; 3]>,
    indices: Vec<[u32; 3]>,
    volume: f64,
    area: f64,
    num_tri: usize,
    /// RGBA 0..1 for shaded preview (None = default warm plastic)
    color: Option<[f32; 4]>,
}

struct App {
    source: String,
    console: String,
    file_path: Option<PathBuf>,
    meshes: Vec<Arc<MeshPreview>>,
    auto_preview: bool,
    dirty: bool,
    last_edit: Instant,
    // view
    yaw: f32,
    pitch: f32,
    zoom: f32,
    last_error: Option<String>,
    show_grid: bool,
    show_axes: bool,
    wireframe: bool,
    status: String,
}

impl App {
    fn new() -> Self {
        Self {
            source: DEFAULT_SCAD.to_string(),
            console: format!(
                "openscad_rs GUI v{} — F5 Preview · Ctrl+O/S/E · drag orbit · scroll zoom\n",
                env!("CARGO_PKG_VERSION")
            ),
            file_path: None,
            meshes: Vec::new(),
            auto_preview: true,
            dirty: true,
            last_edit: Instant::now(),
            yaw: 0.75,
            pitch: 0.45,
            zoom: 1.15,
            last_error: None,
            show_grid: true,
            show_axes: true,
            wireframe: false,
            status: "Ready".into(),
        }
    }

    fn log(&mut self, msg: impl AsRef<str>) {
        self.console.push_str(msg.as_ref());
        if !msg.as_ref().ends_with('\n') {
            self.console.push('\n');
        }
    }

    fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    fn compile(&mut self) {
        self.console.clear();
        let label = self
            .file_path
            .as_ref()
            .map(|p| format!(" {}", p.display()))
            .unwrap_or_default();
        self.log(format!("Compiling{}…", label));
        self.set_status("Compiling…");

        let t0 = Instant::now();
        match openscad_rs::compile(&self.source) {
            Ok(model) => {
                if model.status() != manifold_rust::types::Error::NoError {
                    let e = format!("Geometry error: {:?}", model.status());
                    self.log(&e);
                    self.last_error = Some(e.clone());
                    self.meshes.clear();
                    self.set_status(e);
                    return;
                }
                let mut previews = Vec::new();
                let mut total_tri = 0usize;
                let mut total_vol = 0.0;
                let mut total_area = 0.0;
                for part in &model.parts {
                    if part.solid.num_tri() == 0 {
                        continue;
                    }
                    let mut preview = mesh_from_manifold(&part.solid);
                    preview.color = part.color;
                    total_tri += preview.num_tri;
                    total_vol += preview.volume;
                    total_area += preview.area;
                    previews.push(Arc::new(preview));
                }
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                self.log(format!(
                    "OK  parts={}  triangles={}  volume={:.3}  area={:.3}  ({:.0} ms)",
                    previews.len(), total_tri, total_vol, total_area, ms
                ));
                for (i, p) in model.parts.iter().enumerate() {
                    if let Some(c) = p.color {
                        self.log(format!(
                            "  part[{}] color=({:.2},{:.2},{:.2},{:.2}) tris={}",
                            i, c[0], c[1], c[2], c[3], p.solid.num_tri()
                        ));
                    }
                }
                self.last_error = None;
                self.meshes = previews;
                self.dirty = false;
                self.set_status(format!("OK · {} parts · {:.0} ms", self.meshes.len(), ms));
            }
            Err(e) => {
                let msg = format!("ERROR: {:#}", e);
                self.log(&msg);
                self.last_error = Some(msg.clone());
                self.meshes.clear();
                self.set_status("Error");
            }
        }
    }

    fn export_stl(&mut self) {
        if self.meshes.is_empty() || self.dirty {
            self.compile();
        }
        let model = match openscad_rs::compile(&self.source) {
            Ok(m) => m,
            Err(e) => {
                self.log(format!("Export failed: {:#}", e));
                return;
            }
        };
        let path = if let Some(path) = rfd::FileDialog::new()
            .add_filter("STL", &["stl"])
            .add_filter("USD ASCII (color)", &["usda"])
            .set_file_name("model.stl")
            .save_file()
        {
            path
        } else {
            return;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("stl")
            .to_ascii_lowercase();
        let result = if ext == "usda" || ext == "usd" {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Model");
            openscad_rs::usda::write_usda_model(&model, &path, name)
        } else {
            stl::write_binary_stl(&model.solid(), &path)
        };
        match result {
            Ok(()) => {
                self.log(format!("Wrote {}", path.display()));
                self.set_status(format!(
                    "Exported {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            Err(e) => self.log(format!("Write failed: {:#}", e)),
        }
    }

    fn open_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("OpenSCAD", &["scad"])
            .pick_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(s) => {
                    self.source = s;
                    self.file_path = Some(path.clone());
                    self.dirty = true;
                    self.last_edit = Instant::now();
                    self.log(format!("Opened {}", path.display()));
                    self.compile();
                }
                Err(e) => self.log(format!("Open failed: {}", e)),
            }
        }
    }

    fn save_file(&mut self) {
        let path = if let Some(p) = &self.file_path {
            p.clone()
        } else if let Some(p) = rfd::FileDialog::new()
            .add_filter("OpenSCAD", &["scad"])
            .set_file_name("model.scad")
            .save_file()
        {
            self.file_path = Some(p.clone());
            p
        } else {
            return;
        };
        match std::fs::write(&path, &self.source) {
            Ok(()) => {
                self.log(format!("Saved {}", path.display()));
                self.dirty = false;
                self.set_status(format!("Saved {}", path.file_name().unwrap_or_default().to_string_lossy()));
            }
            Err(e) => self.log(format!("Save failed: {}", e)),
        }
    }

    fn load_example(&mut self, name: &str, code: &str) {
        self.source = code.to_string();
        self.file_path = None;
        self.dirty = true;
        self.last_edit = Instant::now();
        self.log(format!("Loaded example: {}", name));
        self.compile();
    }

    fn reset_view(&mut self) {
        self.yaw = 0.75;
        self.pitch = 0.45;
        self.zoom = 1.15;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-preview after short idle when dirty
        if self.auto_preview && self.dirty && self.last_edit.elapsed() > Duration::from_millis(650) {
            self.compile();
        }

        // Keyboard shortcuts (OpenSCAD-like)
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.compile();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F6)) {
            self.compile();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.open_file();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_file();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::E)) {
            self.export_stl();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
            self.reset_view();
        }

        // Update window title
        let title = match &self.file_path {
            Some(p) => format!(
                "openscad_rs — {}{}",
                p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
                if self.dirty { " *" } else { "" }
            ),
            None => format!("openscad_rs{}", if self.dirty { " *" } else { "" }),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // ── Top menu / toolbar ──────────────────────────────────────────
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open…          Ctrl+O").clicked() {
                        self.open_file();
                        ui.close_menu();
                    }
                    if ui.button("Save           Ctrl+S").clicked() {
                        self.save_file();
                        ui.close_menu();
                    }
                    if ui.button("Export…        Ctrl+E").clicked() {
                        self.export_stl();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Design", |ui| {
                    if ui.button("Preview / Render   F5").clicked() {
                        self.compile();
                        ui.close_menu();
                    }
                    ui.checkbox(&mut self.auto_preview, "Auto-preview");
                });

                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_grid, "Grid");
                    ui.checkbox(&mut self.show_axes, "Axes");
                    ui.checkbox(&mut self.wireframe, "Wireframe");
                    ui.separator();
                    if ui.button("Reset camera   Home").clicked() {
                        self.reset_view();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Examples", |ui| {
                    if ui.button("Tree").clicked() {
                        self.load_example("tree", EXAMPLE_TREE);
                        ui.close_menu();
                    }
                    if ui.button("Box with holes").clicked() {
                        self.load_example("box_with_holes", EXAMPLE_BOX);
                        ui.close_menu();
                    }
                    if ui.button("Difference").clicked() {
                        self.load_example("difference", EXAMPLE_DIFF);
                        ui.close_menu();
                    }
                    if ui.button("Hull + Minkowski").clicked() {
                        self.load_example("hull_minkowski", EXAMPLE_HULL);
                        ui.close_menu();
                    }
                    if ui.button("Modules & for").clicked() {
                        self.load_example("modules", EXAMPLE_MODULES);
                        ui.close_menu();
                    }
                    if ui.button("Extrude").clicked() {
                        self.load_example("extrude", EXAMPLE_EXTRUDE);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    ui.label("openscad_rs — pure Rust OpenSCAD subset");
                    ui.label("Manifold backend · no CGAL");
                    ui.separator();
                    ui.label("F5          Compile & preview");
                    ui.label("Ctrl+O/S/E  Open / Save / Export");
                    ui.label("Home        Reset camera");
                    ui.label("Drag        Orbit");
                    ui.label("Scroll      Zoom");
                });

                ui.separator();

                // Primary actions
                let preview_btn = ui.add(
                    egui::Button::new(egui::RichText::new("▶  Preview").strong())
                        .fill(egui::Color32::from_rgb(45, 110, 70)),
                );
                if preview_btn.clicked() {
                    self.compile();
                }

                if ui
                    .add(
                        egui::Button::new("💾  Export")
                            .fill(egui::Color32::from_rgb(50, 70, 110)),
                    )
                    .clicked()
                {
                    self.export_stl();
                }

                ui.separator();
                ui.checkbox(&mut self.auto_preview, "Auto");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.dirty {
                        ui.colored_label(egui::Color32::from_rgb(230, 180, 60), "● modified");
                    }
                    let title = self
                        .file_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "[unsaved]".into());
                    ui.label(egui::RichText::new(title).weak().small());
                });
            });
        });

        // ── Status bar ──────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status")
            .exact_height(22.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new(&self.status).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.meshes.is_empty() {
                            let tris: usize = self.meshes.iter().map(|m| m.num_tri).sum();
                            let vol: f64 = self.meshes.iter().map(|m| m.volume).sum();
                            let area: f64 = self.meshes.iter().map(|m| m.area).sum();
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} parts · {} tris · V={:.2} · A={:.2}",
                                    self.meshes.len(), tris, vol, area
                                ))
                                .small()
                                .weak(),
                            );
                        }
                    });
                });
            });

        // ── Console ─────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("console")
            .resizable(true)
            .default_height(130.0)
            .min_height(60.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(egui::RichText::new("Console").size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Clear").clicked() {
                            self.console.clear();
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Color error lines red, success green-ish
                        for line in self.console.lines() {
                            let color = if line.starts_with("ERROR") || line.contains("error") {
                                egui::Color32::from_rgb(230, 90, 90)
                            } else if line.starts_with("OK") {
                                egui::Color32::from_rgb(100, 200, 120)
                            } else {
                                egui::Color32::from_rgb(190, 195, 205)
                            };
                            ui.label(egui::RichText::new(line).monospace().color(color).size(12.5));
                        }
                    });
            });

        // ── Left: editor ────────────────────────────────────────────────
        egui::SidePanel::left("editor_panel")
            .resizable(true)
            .default_width(540.0)
            .min_width(280.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(egui::RichText::new("Editor").size(14.0));
                    ui.label(egui::RichText::new(".scad").weak().small());
                });
                ui.add_space(2.0);

                let editor = ui.add(
                    egui::TextEdit::multiline(&mut self.source)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(36)
                        .font(egui::TextStyle::Monospace)
                        .frame(true),
                );
                if editor.changed() {
                    self.dirty = true;
                    self.last_edit = Instant::now();
                }
            });

        // ── Center: 3D preview ──────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("Preview").size(14.0));
                if !self.meshes.is_empty() {
                    let tris: usize = self.meshes.iter().map(|m| m.num_tri).sum();
                    ui.label(
                        egui::RichText::new(format!(
                            "  {} parts · {} tris",
                            self.meshes.len(), tris
                        ))
                        .weak(),
                    );
                } else if let Some(e) = &self.last_error {
                    ui.colored_label(egui::Color32::from_rgb(230, 100, 100), e);
                }
            });

            let (rect, response) = ui.allocate_exact_size(
                ui.available_size(),
                egui::Sense::click_and_drag(),
            );

            // Orbit controls
            if response.dragged() {
                let delta = response.drag_delta();
                self.yaw += delta.x * 0.008;
                self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.45, 1.45);
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    self.zoom = (self.zoom * (1.0 + scroll * 0.0012)).clamp(0.08, 25.0);
                }
            }

            // Dark viewport background
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_rgb(24, 26, 32));

            // Subtle border
            ui.painter().rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(50, 52, 60)),
            );

            if self.show_grid || self.show_axes {
                draw_grid_and_axes(
                    ui.painter(),
                    rect,
                    self.yaw,
                    self.pitch,
                    self.zoom,
                    self.show_grid,
                    self.show_axes,
                );
            }

            if !self.meshes.is_empty() {
                for mesh in &self.meshes {
                    draw_mesh(
                        ui.painter(),
                        rect,
                        mesh,
                        self.yaw,
                        self.pitch,
                        self.zoom,
                        self.wireframe,
                    );
                }
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    if self.auto_preview {
                        "Edit code — auto-preview will run shortly\nor press F5"
                    } else {
                        "Press F5 to preview"
                    },
                    egui::FontId::proportional(16.0),
                    egui::Color32::from_rgb(110, 115, 130),
                );
            }

            // Camera hint
            ui.painter().text(
                rect.left_bottom() + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                "drag = orbit · scroll = zoom · Home = reset",
                egui::FontId::proportional(11.0),
                egui::Color32::from_rgb(100, 105, 120),
            );
        });

        // Keep UI responsive for auto-preview timer
        if self.auto_preview && self.dirty {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
}

fn mesh_from_manifold(model: &Manifold) -> MeshPreview {
    let mesh = model.get_mesh_gl(0);
    let num_prop = (mesh.num_prop as usize).max(3);
    let nvert = mesh.vert_properties.len() / num_prop;
    let mut positions = Vec::with_capacity(nvert);
    for i in 0..nvert {
        let base = i * num_prop;
        positions.push([
            mesh.vert_properties[base],
            mesh.vert_properties[base + 1],
            mesh.vert_properties[base + 2],
        ]);
    }
    let mut indices = Vec::with_capacity(mesh.tri_verts.len() / 3);
    for tri in mesh.tri_verts.chunks(3) {
        if tri.len() == 3 {
            indices.push([tri[0], tri[1], tri[2]]);
        }
    }
    MeshPreview {
        positions,
        indices,
        volume: model.volume(),
        area: model.surface_area(),
        num_tri: model.num_tri() as usize,
        color: None,
    }
}

fn project_point(
    p: [f32; 3],
    center: [f32; 3],
    extent: f32,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    rect: egui::Rect,
) -> (egui::Pos2, f32) {
    let x = (p[0] - center[0]) / extent;
    let y = (p[1] - center[1]) / extent;
    let z = (p[2] - center[2]) / extent;

    let cy = yaw.cos();
    let sy = yaw.sin();
    let cp = pitch.cos();
    let sp = pitch.sin();

    let x1 = x * cy - z * sy;
    let z1 = x * sy + z * cy;
    let y1 = y * cp - z1 * sp;
    let z2 = y * sp + z1 * cp;

    let scale = 0.42 * rect.width().min(rect.height()) * zoom;
    let sx = rect.center().x + x1 * scale;
    let sy = rect.center().y - y1 * scale;
    (egui::pos2(sx, sy), z2)
}

fn draw_grid_and_axes(
    painter: &egui::Painter,
    rect: egui::Rect,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    show_grid: bool,
    show_axes: bool,
) {
    // Fixed world center / scale for grid (independent of model)
    let center = [0.0f32; 3];
    let extent = 40.0;

    if show_grid {
        let grid_color = egui::Color32::from_rgba_unmultiplied(70, 75, 90, 90);
        let major_color = egui::Color32::from_rgba_unmultiplied(90, 95, 110, 130);
        let step = 10.0;
        let n = 4; // -40 .. 40

        for i in -n..=n {
            let t = i as f32 * step;
            let is_major = i == 0;
            let col = if is_major { major_color } else { grid_color };
            let stroke = egui::Stroke::new(if is_major { 1.2_f32 } else { 0.8_f32 }, col);

            // lines parallel to X
            let a = project_point([-40.0, t, 0.0], center, extent, yaw, pitch, zoom, rect);
            let b = project_point([40.0, t, 0.0], center, extent, yaw, pitch, zoom, rect);
            painter.line_segment([a.0, b.0], stroke);

            // lines parallel to Y
            let a = project_point([t, -40.0, 0.0], center, extent, yaw, pitch, zoom, rect);
            let b = project_point([t, 40.0, 0.0], center, extent, yaw, pitch, zoom, rect);
            painter.line_segment([a.0, b.0], stroke);
        }
    }

    if show_axes {
        let origin = [0.0f32; 3];
        let axis_len = 25.0;
        let axes = [
            ([axis_len, 0.0, 0.0], egui::Color32::from_rgb(220, 70, 70), "X"),
            ([0.0, axis_len, 0.0], egui::Color32::from_rgb(70, 200, 90), "Y"),
            ([0.0, 0.0, axis_len], egui::Color32::from_rgb(70, 130, 230), "Z"),
        ];
        let o = project_point(origin, center, extent, yaw, pitch, zoom, rect);
        for (end, color, label) in axes {
            let e = project_point(end, center, extent, yaw, pitch, zoom, rect);
            painter.line_segment([o.0, e.0], egui::Stroke::new(2.0_f32, color));
            painter.text(
                e.0 + egui::vec2(4.0, -4.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(12.0),
                color,
            );
        }
    }
}

fn draw_mesh(
    painter: &egui::Painter,
    rect: egui::Rect,
    mesh: &MeshPreview,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    wireframe: bool,
) {
    if mesh.positions.is_empty() || mesh.indices.is_empty() {
        return;
    }

    // Center & scale to model bounds
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in &mesh.positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let extent = (max[0] - min[0])
        .max(max[1] - min[1])
        .max(max[2] - min[2])
        .max(1e-6);

    // Light direction (world space-ish)
    let light = {
        let l = [0.4f32, 0.7, 0.55];
        let len = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / len, l[1] / len, l[2] / len]
    };

    // Depth-sort triangles (painter's algorithm)
    let mut tris: Vec<(f32, [egui::Pos2; 3], egui::Color32)> =
        Vec::with_capacity(mesh.indices.len());

    for idx in &mesh.indices {
        let p0 = mesh.positions[idx[0] as usize];
        let p1 = mesh.positions[idx[1] as usize];
        let p2 = mesh.positions[idx[2] as usize];

        let (s0, d0) = project_point(p0, center, extent, yaw, pitch, zoom, rect);
        let (s1, d1) = project_point(p1, center, extent, yaw, pitch, zoom, rect);
        let (s2, d2) = project_point(p2, center, extent, yaw, pitch, zoom, rect);

        // Face normal in model space for lighting
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let nx = e1[1] * e2[2] - e1[2] * e2[1];
        let ny = e1[2] * e2[0] - e1[0] * e2[2];
        let nz = e1[0] * e2[1] - e1[1] * e2[0];
        let nlen = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-12);
        let n = [nx / nlen, ny / nlen, nz / nlen];

        let ndotl = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).clamp(0.0, 1.0);
        // Soft ambient + diffuse, OpenSCAD-ish yellow/cream plastic
        let amb = 0.28;
        let diff = 0.72 * ndotl;
        let shade = amb + diff;
        let base = mesh.color.unwrap_or([0.82, 0.68, 0.37, 1.0]);
        let r = (base[0] * 255.0 * shade).clamp(0.0, 255.0) as u8;
        let g = (base[1] * 255.0 * shade).clamp(0.0, 255.0) as u8;
        let b = (base[2] * 255.0 * shade).clamp(0.0, 255.0) as u8;
        let a = (base[3] * 255.0).clamp(0.0, 255.0) as u8;
        let color = if a < 250 {
            egui::Color32::from_rgba_unmultiplied(r, g, b, a)
        } else {
            egui::Color32::from_rgb(r, g, b)
        };

        let depth = (d0 + d1 + d2) / 3.0;
        tris.push((depth, [s0, s1, s2], color));
    }

    // Far to near
    tris.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    if wireframe {
        let stroke = egui::Stroke::new(0.9_f32, egui::Color32::from_rgb(180, 160, 90));
        for (_, pts, _) in &tris {
            painter.line_segment([pts[0], pts[1]], stroke);
            painter.line_segment([pts[1], pts[2]], stroke);
            painter.line_segment([pts[2], pts[0]], stroke);
        }
    } else {
        for (_, pts, color) in &tris {
            painter.add(egui::Shape::convex_polygon(
                pts.to_vec(),
                *color,
                egui::Stroke::new(0.35_f32, egui::Color32::from_rgba_unmultiplied(30, 25, 15, 60)),
            ));
        }
    }
}

// ── Default / example sources ───────────────────────────────────────────

const DEFAULT_SCAD: &str = r#"// openscad_rs — multi-color preview
// Press F5 (or enable Auto) · Ctrl+E to export STL / USDA

$fn = 48;

color("brown")
    cylinder(h = 30, r = 6);

color("green")
    translate([0, 0, 35])
        sphere(r = 16);

color("steelblue")
    translate([0, 0, -2])
        cylinder(h = 4, r = 14);

color("gold")
    translate([18, 0, 5])
        cube(8, center = true);
"#;

const EXAMPLE_TREE: &str = r#"// Classic tree
$fn = 32;
union() {
    cylinder(h = 30, r = 8);
    translate([0, 0, 40]) sphere(20);
}
"#;

const EXAMPLE_BOX: &str = r#"// Box with cylindrical holes
$fn = 36;
difference() {
    cube([30, 20, 10], center = true);
    translate([8, 0, 0]) cylinder(h = 20, r = 3, center = true);
    translate([-8, 0, 0]) cylinder(h = 20, r = 3, center = true);
}
"#;

const EXAMPLE_DIFF: &str = r#"// Sphere subtracted from cube
$fn = 40;
difference() {
    cube(20, center = true);
    sphere(12);
}
"#;

const EXAMPLE_HULL: &str = r#"// Hull of two spheres + minkowski tip
$fn = 28;
union() {
    hull() {
        translate([-12, 0, 0]) sphere(6);
        translate([12, 0, 0]) sphere(6);
    }
    translate([0, 0, 12])
        minkowski() {
            cube([6, 6, 2], center = true);
            sphere(2);
        }
}
"#;

const EXAMPLE_MODULES: &str = r#"// Modules + for-loop
$fn = 24;

module peg(h = 10, r = 2) {
    cylinder(h = h, r = r);
}

for (i = [0 : 4]) {
    translate([i * 12 - 24, 0, 0])
        peg(h = 8 + i * 3, r = 2.5);
}
"#;

const EXAMPLE_EXTRUDE: &str = r#"// Linear extrude of a square
$fn = 32;
linear_extrude(height = 15)
    square([12, 8], center = true);

translate([25, 0, 0])
    rotate_extrude()
        translate([8, 0, 0])
            square([3, 6]);
"#;
