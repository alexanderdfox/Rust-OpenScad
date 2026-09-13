//! OpenSCAD-like GUI: editor, 3D preview, console, render/export.

use eframe::egui;
use manifold_rust::manifold::Manifold;
use openscad_rs::stl;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("openscad_rs"),
        ..Default::default()
    };
    eframe::run_native(
        "openscad_rs",
        options,
        Box::new(|cc| {
            egui_extras_install(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

fn egui_extras_install(_ctx: &egui::Context) {
    // placeholder for fonts if needed later
}

struct MeshPreview {
    /// packed xyz xyz ...
    positions: Vec<[f32; 3]>,
    /// triangle indices
    indices: Vec<[u32; 3]>,
    volume: f64,
    area: f64,
    num_tri: usize,
}

struct App {
    source: String,
    console: String,
    file_path: Option<PathBuf>,
    mesh: Option<Arc<MeshPreview>>,
    auto_preview: bool,
    dirty: bool,
    // view
    yaw: f32,
    pitch: f32,
    zoom: f32,
    last_error: Option<String>,
}

impl App {
    fn new() -> Self {
        Self {
            source: DEFAULT_SCAD.to_string(),
            console: "openscad_rs GUI — F5 Preview · F6 Render & export ready\n".into(),
            file_path: None,
            mesh: None,
            auto_preview: false,
            dirty: true,
            yaw: 0.6,
            pitch: 0.5,
            zoom: 1.0,
            last_error: None,
        }
    }

    fn log(&mut self, msg: impl AsRef<str>) {
        self.console.push_str(msg.as_ref());
        if !msg.as_ref().ends_with('\n') {
            self.console.push('\n');
        }
    }

    fn compile(&mut self) {
        self.console.clear();
        self.log(format!("Compiling{}…", self.file_path.as_ref().map(|p| format!(" {}", p.display())).unwrap_or_default()));
        match openscad_rs::compile(&self.source) {
            Ok(model) => {
                if model.status() != manifold_rust::types::Error::NoError {
                    let e = format!("Geometry error: {:?}", model.status());
                    self.log(&e);
                    self.last_error = Some(e);
                    self.mesh = None;
                    return;
                }
                let preview = mesh_from_manifold(&model);
                self.log(format!(
                    "OK  triangles={}  volume={:.3}  area={:.3}",
                    preview.num_tri, preview.volume, preview.area
                ));
                self.last_error = None;
                self.mesh = Some(Arc::new(preview));
                self.dirty = false;
            }
            Err(e) => {
                let msg = format!("ERROR: {:#}", e);
                self.log(&msg);
                self.last_error = Some(msg);
                self.mesh = None;
            }
        }
    }

    fn export_stl(&mut self) {
        if self.mesh.is_none() || self.dirty {
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
            .set_file_name("model.stl")
            .save_file()
        {
            path
        } else {
            return;
        };
        match stl::write_binary_stl(&model, &path) {
            Ok(()) => self.log(format!("Wrote {}", path.display())),
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
            }
            Err(e) => self.log(format!("Save failed: {}", e)),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open…    Ctrl+O").clicked() {
                        self.open_file();
                        ui.close_menu();
                    }
                    if ui.button("Save     Ctrl+S").clicked() {
                        self.save_file();
                        ui.close_menu();
                    }
                    if ui.button("Export STL…  Ctrl+E").clicked() {
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
                });
                ui.menu_button("Help", |ui| {
                    ui.label("openscad_rs — Rust OpenSCAD subset");
                    ui.label("Manifold backend · no CGAL");
                    ui.label("F5 = compile & preview");
                    ui.label("Drag in 3D view to rotate · scroll to zoom");
                });

                ui.separator();
                if ui.button("▶ Preview (F5)").clicked() {
                    self.compile();
                }
                if ui.button("💾 Export STL").clicked() {
                    self.export_stl();
                }
                ui.separator();
                let title = self
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "[unsaved]".into());
                ui.label(egui::RichText::new(title).weak());
                if self.dirty {
                    ui.colored_label(egui::Color32::YELLOW, "• modified");
                }
            });
        });

        egui::TopBottomPanel::bottom("console")
            .resizable(true)
            .default_height(140.0)
            .show(ctx, |ui| {
                ui.heading("Console");
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.monospace(&self.console);
                    });
            });

        // Left: editor, Right: 3D view
        egui::SidePanel::left("editor_panel")
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.heading("Editor");
                let editor = ui.add(
                    egui::TextEdit::multiline(&mut self.source)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(40)
                        .font(egui::TextStyle::Monospace),
                );
                if editor.changed() {
                    self.dirty = true;
                }
                if ui.input(|i| i.pointer.any_pressed()) {
                    // mark dirty when editing - egui doesn't easily tell us; check via response
                }
            });

        // Track edits
        // (re-show to get response is awkward; set dirty on any source change via hash next frame)
        // Simple: always dirty after any frame the text edit had focus change

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Preview");
                if let Some(m) = &self.mesh {
                    ui.label(format!(
                        "  {} tris  V={:.2}  A={:.2}",
                        m.num_tri, m.volume, m.area
                    ));
                } else if let Some(e) = &self.last_error {
                    ui.colored_label(egui::Color32::LIGHT_RED, e);
                }
            });

            let (rect, response) = ui.allocate_exact_size(
                ui.available_size(),
                egui::Sense::click_and_drag(),
            );

            // Orbit controls
            if response.dragged() {
                let delta = response.drag_delta();
                self.yaw += delta.x * 0.01;
                self.pitch = (self.pitch + delta.y * 0.01).clamp(-1.4, 1.4);
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.1, 20.0);
                }
            }

            // Background
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_rgb(30, 32, 40));

            if let Some(mesh) = &self.mesh {
                draw_mesh(
                    ui.painter(),
                    rect,
                    mesh,
                    self.yaw,
                    self.pitch,
                    self.zoom,
                );
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Press F5 to preview",
                    egui::FontId::proportional(18.0),
                    egui::Color32::GRAY,
                );
            }

            // Axes hint
            ui.painter().text(
                rect.left_bottom() + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                "drag = orbit · scroll = zoom",
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(140),
            );
        });

        // Detect source changes cheaply
        // mark dirty when editor loses change - use memory
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
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
    }
}

fn draw_mesh(
    painter: &egui::Painter,
    rect: egui::Rect,
    mesh: &MeshPreview,
    yaw: f32,
    pitch: f32,
    zoom: f32,
) {
    if mesh.positions.is_empty() || mesh.indices.is_empty() {
        return;
    }

    // Center & scale
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

    let cy = yaw.cos();
    let sy = yaw.sin();
    let cp = pitch.cos();
    let sp = pitch.sin();

    // Project function
    let project = |p: [f32; 3]| -> (egui::Pos2, f32) {
        let x = (p[0] - center[0]) / extent;
        let y = (p[1] - center[1]) / extent;
        let z = (p[2] - center[2]) / extent;
        // yaw then pitch
        let x1 = x * cy - z * sy;
        let z1 = x * sy + z * cy;
        let y1 = y * cp - z1 * sp;
        let z2 = y * sp + z1 * cp;
        let scale = 0.45 * rect.width().min(rect.height()) * zoom;
        let sx = rect.center().x + x1 * scale;
        let sy = rect.center().y - y1 * scale;
        (egui::pos2(sx, sy), z2)
    };

    // Depth-sort triangles (painter's algorithm)
    let mut tris: Vec<(f32, [egui::Pos2; 3], egui::Color32)> = Vec::with_capacity(mesh.indices.len());
    for idx in &mesh.indices {
        let (p0, d0) = project(mesh.positions[idx[0] as usize]);
        let (p1, d1) = project(mesh.positions[idx[1] as usize]);
        let (p2, d2) = project(mesh.positions[idx[2] as usize]);
        let depth = (d0 + d1 + d2) / 3.0;
        // simple lighting from face normal z in view space
        let e1 = [p1.x - p0.x, p1.y - p0.y];
        let e2 = [p2.x - p0.x, p2.y - p0.y];
        let cross = e1[0] * e2[1] - e1[1] * e2[0];
        if cross >= 0.0 {
            continue; // back-face cull
        }
        let light = (cross.abs() / 2000.0).clamp(0.25, 1.0);
        let c = (40.0 + 160.0 * light) as u8;
        let color = egui::Color32::from_rgb(c, c + 20, c + 40);
        tris.push((depth, [p0, p1, p2], color));
    }
    tris.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, pts, color) in tris {
        painter.add(egui::Shape::convex_polygon(
            pts.to_vec(),
            color,
            egui::Stroke::new(0.4, egui::Color32::from_rgb(20, 20, 30)),
        ));
    }
}

const DEFAULT_SCAD: &str = r#"// openscad_rs sample
$fn = 48;

difference() {
    union() {
        cube([30, 20, 10], center = true);
        translate([0, 0, 10])
            cylinder(h = 15, r = 6);
    }
    translate([0, 0, -1])
        cylinder(h = 30, r = 3);
}
"#;
