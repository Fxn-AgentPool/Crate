//! Windowed application: panels, viewport interaction and shortcuts.

use std::path::PathBuf;

use egui::{Key, Modifiers, PointerButton, Sense};
use glam::Vec2;

use crate::camera::Camera;
use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::mesh::{self, Diagnostic, ImportSettings, Severity, TangentMode, UpAxis};
use crate::meshmaps::{MapStatus, MeshMap};
use crate::project::Project;
use crate::paint::{BakeMode, PaintEngine, Tool};
use crate::ui_layers::{self, PanelActions};
use crate::viewport::{DisplayMode, ViewSettings, Viewport};

struct NewProjectDialog {
    mesh_path: Option<PathBuf>,
    settings: ImportSettings,
    error: Option<String>,
}

impl NewProjectDialog {
    fn new() -> Self {
        Self { mesh_path: None, settings: ImportSettings::default(), error: None }
    }
}

pub struct CrateApp {
    gpu: Gpu,
    renderer: std::sync::Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    viewport: Viewport,
    viewport_texture: Option<(egui::TextureId, u32, u32)>,
    doc: Option<Project>,
    camera: Camera,
    view: ViewSettings,
    new_project: Option<NewProjectDialog>,
    show_import_log: bool,
    show_mesh_maps: bool,
    bake_pane: crate::ui_bake_preview::BakePane,
    /// Diagnostics of an import that failed, shown in the Import Log.
    failed_import_log: Option<Vec<Diagnostic>>,
    /// Maps checked in the Mesh Maps window.
    bake_selection: [bool; 8],
    error: Option<String>,
    /// Material version compiled into the viewport.
    viewport_material: u64,
    /// Transient message in the status bar.
    message: Option<(String, f64)>,
    /// The GPU lacks something Crate needs; shown instead of the UI.
    gpu_error: Option<String>,
    paint: PaintEngine,
    stencil_overlay: crate::stencil::StencilOverlay,
    /// Whether S / V was used for a drag since it was pressed (tap S toggles visibility).
    s_dragged: bool,
    export_dialog: crate::ui_export::ExportDialog,
    evaluator: crate::export::UvEvaluator,
    /// Action waiting for the user to decide about unsaved changes.
    pending: Option<Pending>,
    allow_close: bool,
    node_editor: crate::ui_graph::NodeEditor,
    windows: crate::ui_windows::Windows,
    prefs: crate::prefs::Preferences,
    library: crate::ui_library::Library,
    environment: Option<PathBuf>,
    bindings: crate::shortcuts::Bindings,
    thumbs: crate::thumbs::Thumbnails,
    updater: crate::ui_update::Updater,
    /// Start the program again on exit (after an update).
    restart: bool,
    /// File > Open Recent.
    recent: crate::recent::Recent,
}

#[derive(Clone)]
enum Pending {
    New,
    Open,
    OpenRecent(PathBuf),
    Quit,
    /// Quit, then start the version an update installed.
    Restart,
}

impl CrateApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_mesh: Option<PathBuf>, script: Option<PathBuf>) -> Self {
        apply_theme(&cc.egui_ctx);
        let rs = cc.wgpu_render_state.as_ref().expect("Crate requires the wgpu renderer");
        let gpu = Gpu { device: rs.device.clone(), queue: rs.queue.clone() };
        let viewport = Viewport::new(&gpu, &crate::scene::layout(&gpu));
        let paint = PaintEngine::new(&gpu);
        let paint_gpu = gpu.clone();
        let prefs_for_bindings = crate::prefs::Preferences::load();
        let mut app = Self {
            gpu,
            renderer: rs.renderer.clone(),
            viewport,
            viewport_texture: None,
            doc: None,
            camera: Camera::default(),
            bake_pane: crate::ui_bake_preview::BakePane::new(),
            view: ViewSettings::default(),
            new_project: None,
            show_import_log: false,
            show_mesh_maps: false,
            failed_import_log: None,
            bake_selection: [false; 8],
            error: None,
            viewport_material: u64::MAX,
            message: None,
            gpu_error: crate::gpu::check_adapter(&rs.adapter).err(),
            stencil_overlay: crate::stencil::StencilOverlay::new(&paint_gpu),
            evaluator: crate::export::UvEvaluator::new(&paint_gpu),
            paint,
            s_dragged: false,
            export_dialog: Default::default(),
            pending: None,
            allow_close: false,
            node_editor: Default::default(),
            windows: Default::default(),
            prefs: crate::prefs::Preferences::load(),
            library: Default::default(),
            environment: None,
            bindings: crate::shortcuts::Bindings::new(&prefs_for_bindings.shortcuts),
            thumbs: crate::thumbs::Thumbnails::new(&paint_gpu),
            updater: Default::default(),
            restart: false,
            recent: crate::recent::Recent::load(),
        };
        if let Err(e) = crate::library::ensure_folders() {
            log::warn!("{e:#}");
        }
        if app.prefs.check_updates_at_startup {
            app.updater.check(&cc.egui_ctx, true);
        }
        if app.gpu_error.is_some() {
            return app;
        }
        if let Some(path) = initial_mesh {
            // A .crate directory opens as a project; anything else is imported as a mesh.
            if path.is_dir() {
                app.open_project(&path);
            } else {
                app.open_mesh(path, ImportSettings::default());
            }
        }
        if let Some(script) = script {
            app.run_startup_script(&script);
        }
        app
    }

    fn open_mesh(&mut self, path: PathBuf, settings: ImportSettings) -> bool {
        let mesh = match mesh::import(&path, &settings) {
            Ok(mesh) => mesh,
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                return false;
            }
        };
        if mesh.has_errors() {
            self.show_import_log = true;
            self.error = Some("Import aborted. See the Import Log.".into());
            self.failed_import_log = Some(mesh.diagnostics);
            return false;
        }
        if mesh.diagnostics.iter().any(|d| d.severity >= Severity::Warning) {
            self.show_import_log = true;
        }
        self.camera = Camera::default();
        self.camera.frame(mesh.center(), mesh.radius());
        self.view = ViewSettings::default();
        let project = Project::new(&self.gpu, mesh, settings);
        self.paint = PaintEngine::new(&self.gpu);
        self.viewport = Viewport::new(&self.gpu, &project.scene.layout);
        self.viewport.set_mesh(&self.gpu, &project.gpu_mesh);
        self.view.height_scale = project.channels.height_scale;
        self.bake_selection = MeshMap::ALL.map(|m| project.maps.status(m) != MapStatus::UpToDate);
        self.doc = Some(project);
        self.thumbs.clear();
        self.library.project_changed();
        self.viewport_material = u64::MAX;
        self.failed_import_log = None;
        self.error = None;
        true
    }
}

impl CrateApp {
    /// Runs a script (see script.rs) and adopts the project it creates.
    fn run_startup_script(&mut self, path: &std::path::Path) {
        let base = std::env::current_dir().unwrap_or_default();
        let mut session = crate::script::Session::with_gpu(self.gpu.clone(), base);
        session.project = self.doc.take();
        if let Err(e) = session.run_file(path) {
            self.error = Some(format!("Startup script failed: {e:#}"));
        }
        if let Some(project) = session.project.take() {
            self.viewport = Viewport::new(&self.gpu, &project.scene.layout);
            self.viewport.set_mesh(&self.gpu, &project.gpu_mesh);
            self.view = session.view;
            self.view.height_scale = project.channels.height_scale;
            self.camera = session.camera;
            self.paint = session.paint;
            self.doc = Some(project);
            self.thumbs.clear();
            self.library.project_changed();
            self.viewport_material = u64::MAX;
        }
    }
}

impl eframe::App for CrateApp {
    fn on_exit(&mut self) {
        if !self.restart {
            return;
        }
        // The update put the new program where this one was started from.
        match crate::update::program_path().ok_or("can't locate the program") {
            Ok(exe) => {
                let mut cmd = std::process::Command::new(exe);
                if let Some(path) = self.doc.as_ref().and_then(|d| d.path.clone()) {
                    cmd.arg(path);
                }
                if let Err(e) = cmd.spawn() {
                    log::error!("couldn't restart Crate: {e}");
                }
            }
            Err(e) => log::error!("couldn't restart Crate: {e}"),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(e) = &self.gpu_error {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.35);
                    ui.heading("Unsupported GPU");
                    ui.add_space(8.0);
                    ui.label(e);
                });
            });
            return;
        }
        if let Some(doc) = &mut self.doc {
            if doc.tick(&self.gpu, std::time::Duration::from_millis(12)) {
                ui.ctx().request_repaint();
            }
            // The bake preview is part of the Mesh Maps window; whenever the window opens, its
            // camera looks from the viewport's direction and frames the whole mesh.
            if self.show_mesh_maps && !doc.maps.preview.open {
                self.bake_pane.camera = self.camera;
                self.bake_pane.frame(&doc.mesh);
            }
            doc.maps.preview.open = self.show_mesh_maps;
            doc.update_preview(&self.gpu);
            match doc.maps.report.take() {
                Some(Ok(msg)) => self.message = Some((msg, ui.input(|i| i.time))),
                Some(Err(e)) => self.error = Some(e),
                None => {}
            }
        }
        self.shortcuts(ui.ctx());
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.doc.as_ref().is_some_and(|d| d.has_unsaved_changes()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Quit);
            }
        }
        let title = match &self.doc {
            Some(d) => format!(
                "{}{} - Crate",
                d.name.clone().unwrap_or_else(|| "Untitled".into()),
                if d.has_unsaved_changes() { " *" } else { "" }
            ),
            None => "Crate".into(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        self.unsaved_dialog(&ctx);
        self.menu_bar(ui);
        self.status_bar(ui);
        self.side_panels(ui);
        egui::CentralPanel::no_frame().show(ui, |ui| self.viewport_ui(ui));
        self.dialogs(ui.ctx());
    }
}

impl CrateApp {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use crate::shortcuts::Action as A;
        if ctx.egui_wants_keyboard_input() || self.windows.capturing_shortcut() {
            return;
        }
        let bindings = self.bindings.clone();
        let (actions, time, stencil_key, buffer_key) = ctx.input_mut(|i| {
            let s_key = bindings.key(A::StencilKey);
            let v_key = bindings.key(A::BufferKey);
            // Hold-key gestures (S / V by default) are handled here, before shortcuts.
            if i.key_pressed(s_key) && !i.modifiers.any() {
                self.s_dragged = false;
            }
            let s_down = i.key_down(s_key);
            let flip = s_down && i.key_pressed(Key::H);
            let reset_s = s_down && i.key_pressed(Key::R);
            let toggle_s = i.key_released(s_key) && !self.s_dragged && !i.modifiers.alt;
            let reset_v = i.key_down(v_key) && i.key_pressed(Key::R);
            if flip || reset_s {
                self.s_dragged = true;
            }
            let escape = self.node_editor.open && !self.node_editor.has_popup() && i.consume_key(Modifiers::NONE, Key::Escape);
            // Over the Node Editor, its own keys (Delete, Tab, Home, …) win over the viewport's.
            let over_editor = self.node_editor.open && i.pointer.hover_pos().is_some_and(|p| self.node_editor.rect.contains(p));
            let allow = |a: A| {
                !over_editor || matches!(a, A::ToggleNodeEditor | A::Undo | A::Redo | A::Save | A::NewProject | A::OpenProject | A::ExportTextures)
            };
            let mut actions = if s_down || i.key_down(v_key) { Vec::new() } else { bindings.triggered(i, allow) };
            if escape {
                self.node_editor.open = false;
            }
            (std::mem::take(&mut actions), i.time, (flip, reset_s, toggle_s), reset_v)
        });
        let mut file_action = None;
        let mut save_now = false;
        for a in &actions {
            match a {
                A::NewProject => file_action = Some(Pending::New),
                A::OpenProject => file_action = Some(Pending::Open),
                A::Save => save_now = true,
                _ => {}
            }
        }
        if let Some(doc) = &mut self.doc {
            let gpu = &self.gpu;
            let (flip, reset_s, toggle_s) = stencil_key;
            if let Some(st) = doc.stencils.active_mut() {
                if flip {
                    st.flip = !st.flip;
                }
                if reset_s {
                    st.reset_transform();
                }
                if toggle_s {
                    st.visible = !st.visible;
                }
            }
            if buffer_key {
                self.paint.transform = Default::default();
            }
            for a in actions {
                match a {
                    A::Undo => {
                        if let Some(l) = doc.undo(gpu) {
                            self.message = Some((format!("Undo: {l}"), time));
                        }
                    }
                    A::Redo => {
                        if let Some(l) = doc.redo(gpu) {
                            self.message = Some((format!("Redo: {l}"), time));
                        }
                    }
                    A::ExportTextures => self.export_dialog.open = true,
                    A::ToggleBakeMode => {
                        let mode = if self.paint.mode == BakeMode::Live { BakeMode::Manual } else { BakeMode::Live };
                        self.paint.set_mode(gpu, doc, mode);
                    }
                    A::Bake => {
                        if let Err(e) = self.paint.bake(gpu, doc) {
                            self.message = Some((e, time));
                        }
                    }
                    A::ClearBuffer => self.paint.clear_buffer(gpu),
                    A::ToolPaint => self.paint.tool = Tool::Paint,
                    A::ToolErase => self.paint.tool = Tool::Erase,
                    A::ToolPaintThrough => self.paint.tool = Tool::PaintThrough,
                    A::SofterBrush => self.paint.brush.hardness = (self.paint.brush.hardness - 0.1).max(0.0),
                    A::HarderBrush => self.paint.brush.hardness = (self.paint.brush.hardness + 0.1).min(1.0),
                    A::SmallerBrush => self.paint.brush.size = (self.paint.brush.size / 1.15).max(1.0),
                    A::LargerBrush => self.paint.brush.size = (self.paint.brush.size * 1.15).min(2000.0),
                    A::NewPaintLayer => ui_layers::add_paint(gpu, doc),
                    A::NewFillLayer => ui_layers::add_fill(gpu, doc),
                    A::Group => ui_layers::group_selected(gpu, doc),
                    A::Duplicate => ui_layers::duplicate_selected(gpu, doc),
                    A::Delete => ui_layers::delete_selected(gpu, doc),
                    A::NextStencil => doc.stencils.next(),
                    A::EditAsGraph => {
                        if let Some(sel) = doc.selection {
                            self.node_editor.open_for(sel.layer());
                        }
                    }
                    A::ToggleNodeEditor => self.node_editor.toggle(doc.selection.map(|s| s.layer())),
                    A::FrameMesh => self.camera.frame(doc.mesh.center(), doc.mesh.radius()),
                    A::TileGrid => self.view.show_tiles = !self.view.show_tiles,
                    A::Wireframe => self.view.wireframe = !self.view.wireframe,
                    A::MaterialView => self.view.display = DisplayMode::Material,
                    A::CycleMeshMaps => {
                        self.view.display = match self.view.display {
                            DisplayMode::MeshMap(m) => DisplayMode::MeshMap((m + 1) % MeshMap::ALL.len() as u32),
                            _ => DisplayMode::MeshMap(0),
                        }
                    }
                    A::CycleChannels => {
                        self.view.display = match self.view.display {
                            DisplayMode::Channel(c) => DisplayMode::Channel((c + 1) % Channel::ALL.len() as u32),
                            _ => DisplayMode::Channel(0),
                        }
                    }
                    _ => {}
                }
            }
        }
        self.after_shortcuts(file_action, save_now);
    }

    fn after_shortcuts(&mut self, file_action: Option<Pending>, save_now: bool) {
        if let Some(a) = file_action {
            self.guarded(a);
        }
        if save_now && self.doc.is_some() {
            self.save();
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project…\tCtrl+N").clicked() {
                        self.guarded(Pending::New);
                    }
                    if ui.button("Open Project…\tCtrl+O").clicked() {
                        self.guarded(Pending::Open);
                    }
                    ui.menu_button("Open Recent", |ui| self.recent_menu(ui));
                    if ui.add_enabled(self.doc.is_some(), egui::Button::new("Save\tCtrl+S")).clicked() {
                        self.save();
                    }
                    if ui.add_enabled(self.doc.is_some(), egui::Button::new("Save As…")).clicked() {
                        self.save_as();
                    }
                    if ui.add_enabled(self.doc.is_some(), egui::Button::new("Reimport Mesh…")).clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("Mesh", &["glb", "gltf", "obj"]).pick_file() {
                            if let Some(doc) = &mut self.doc {
                                match doc.reimport(&self.gpu, &path) {
                                    Ok(()) => {
                                        self.viewport = Viewport::new(&self.gpu, &doc.scene.layout);
                                        self.viewport.set_mesh(&self.gpu, &doc.gpu_mesh);
                                        self.viewport_material = u64::MAX;
                                        self.paint = PaintEngine::new(&self.gpu);
                                        self.thumbs.clear();
                                        self.library.project_changed();
                                        self.show_import_log |= !doc.mesh.diagnostics.is_empty();
                                    }
                                    Err(e) => self.error = Some(format!("Reimport failed: {e:#}")),
                                }
                            }
                        }
                    }
                    ui.separator();
                    if ui.add_enabled(self.doc.is_some(), egui::Button::new("Export Textures…\tCtrl+E")).clicked() {
                        self.export_dialog.open = true;
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Preferences…").clicked() {
                        self.windows.preferences = true;
                    }
                    if let Some(doc) = &mut self.doc {
                        if ui.button("Undo	Ctrl+Z").clicked() {
                            doc.undo(&self.gpu);
                        }
                        if ui.button("Redo	Ctrl+Shift+Z").clicked() {
                            doc.redo(&self.gpu);
                        }
                    }
                });
                // Sliders and the transform list: only a click outside closes this menu (egui's
                // default closes on any click, including on a slider).
                let keep_open = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                egui::containers::menu::MenuButton::new("Viewport").config(keep_open).ui(ui, |ui| {
                    ui.menu_button(format!("View transform: {}", self.view.view_transform.name()), |ui| {
                        for t in crate::viewport::ViewTransform::ALL {
                            ui.radio_value(&mut self.view.view_transform, t, t.name());
                        }
                    });
                    ui.add(egui::Slider::new(&mut self.view.exposure, -5.0..=5.0).text("Exposure (EV)"));
                    ui.add(egui::Slider::new(&mut self.view.env_rotation, -3.15..=3.15).text("Environment rotation"));
                    if ui.button("Load environment (HDR/EXR)…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("Environment", &["hdr", "exr"]).pick_file() {
                            match self.viewport.set_environment(&self.gpu, Some(&path)) {
                                Ok(()) => {
                                    self.view.hdri = true;
                                    self.environment = Some(path);
                                }
                                Err(e) => self.error = Some(format!("{e:#}")),
                            }
                        }
                        ui.close();
                    }
                    if ui.add_enabled(self.view.hdri, egui::Button::new("Studio lighting")).clicked() {
                        let _ = self.viewport.set_environment(&self.gpu, None);
                        self.view.hdri = false;
                        self.environment = None;
                        ui.close();
                    }
                });
                ui.menu_button("Project", |ui| {
                    ui.add_enabled_ui(self.doc.is_some(), |ui| {
                        ui.checkbox(&mut self.windows.channels, "Channels");
                    });
                });
                ui.menu_button("Window", |ui| {
                    ui.checkbox(&mut self.windows.performance, "Performance");
                    ui.checkbox(&mut self.node_editor.open, "Node Editor");
                    ui.checkbox(&mut self.show_mesh_maps, "Mesh Maps");
                    ui.checkbox(&mut self.show_import_log, "Import Log");
                });
                ui.menu_button("Help", |ui| {
                    let label = if self.updater.available().is_some() { "Update Available…" } else { "Check for Updates…" };
                    if ui.button(label).clicked() {
                        self.updater.open = true;
                        self.updater.check(ui.ctx(), false);
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                    ui.separator();
                }
                if let Some(v) = self.updater.available() {
                    let r = ui.add(egui::Label::new(egui::RichText::new(format!("Crate {v} is available")).color(egui::Color32::from_rgb(110, 200, 120))).sense(Sense::click()));
                    if r.on_hover_text("Help → Check for Updates…").clicked() {
                        self.updater.open = true;
                    }
                    ui.separator();
                }
                let Some(doc) = &self.doc else {
                    ui.label("No project. File → New Project (Ctrl+N).");
                    return;
                };
                let m = &doc.mesh;
                ui.label(format!(
                    "{} · {} triangles · tiles {} · {}²",
                    doc.original_mesh.file_name().unwrap_or_default().to_string_lossy(),
                    m.triangle_count(),
                    m.tiles.iter().map(u32::to_string).collect::<Vec<_>>().join(", "),
                    doc.tile_resolution(),
                ));
                ui.separator();
                ui.label(if self.paint.mode == BakeMode::Live { "Live" } else { "Manual" });
                if self.paint.unbaked {
                    ui.colored_label(egui::Color32::from_rgb(240, 150, 40), "Unbaked");
                }
                ui.separator();
                ui.label(paint_target_label(doc));
                ui.separator();
                ui.label(match self.view.display {
                    DisplayMode::Material => "Material".to_string(),
                    DisplayMode::Channel(c) => Channel::ALL[c as usize].name().to_string(),
                    DisplayMode::MeshMap(i) => MeshMap::ALL[i as usize].name().to_string(),
                });
                if !doc.material_errors.is_empty() {
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), doc.material_errors.join("; "));
                }
                if let Some((msg, t)) = &self.message {
                    if ui.input(|i| i.time) - t < 3.0 {
                        ui.separator();
                        ui.label(msg);
                    }
                }
                if let Some(job) = &doc.bake_job {
                    ui.separator();
                    ui.add(
                        egui::ProgressBar::new(job.progress())
                            .desired_width(160.0)
                            .text("Baking mesh maps"),
                    );
                }
            });
        });
    }

    fn viewport_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let Some(doc) = self.doc.as_mut() else {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(40));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "File → New Project (Ctrl+N) to import a mesh",
                egui::FontId::proportional(16.0),
                egui::Color32::from_gray(160),
            );
            return;
        };

        let ppp = ui.ctx().pixels_per_point();
        let (w, h) = ((rect.width() * ppp) as u32, (rect.height() * ppp) as u32);
        if w == 0 || h == 0 {
            return;
        }
        let gpu_owned = self.gpu.clone();
        let gpu = &gpu_owned;
        let input = ui.input(|i| {
            let pressure = i
                .events
                .iter()
                .rev()
                .find_map(|e| match e {
                    egui::Event::Touch { force: Some(f), .. } => Some(*f),
                    _ => None,
                })
                .unwrap_or(1.0);
            (
                i.modifiers,
                i.pointer.delta(),
                [
                    i.pointer.button_down(PointerButton::Primary),
                    i.pointer.button_down(PointerButton::Middle),
                    i.pointer.button_down(PointerButton::Secondary),
                ],
                i.smooth_scroll_delta.y,
                pressure,
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.hover_pos(),
                i.key_down(self.bindings.key(crate::shortcuts::Action::BufferKey)),
                i.time,
                i.key_down(self.bindings.key(crate::shortcuts::Action::StencilKey)),
            )
        });
        let (mods, delta, buttons, scroll, pressure, pressed, released, hover, v_down, time, s_down) = input;
        let stencil = doc.stencils.sample(|n| doc.scene.image_layer(n), (w, h));
        let d = Vec2::new(delta.x, delta.y);
        let to_px = |p: egui::Pos2| Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
        let active = response.dragged() || response.is_pointer_button_down_on();

        if self.paint.stroking() {
            // Strokes continue until the button is released.
            if let Some(hp) = hover {
                self.paint.stroke_to(gpu, to_px(hp), pressure);
            }
            if released || !buttons[0] {
                self.paint.end_stroke(gpu, doc, stencil);
            }
        } else if active && s_down && buttons[0] {
            // Hold S: move, Shift rotates, Ctrl scales the active stencil.
            self.s_dragged = true;
            if let Some(s) = doc.stencils.active_mut() {
                if mods.shift {
                    s.rotation += d.x * 0.01;
                } else if mods.command {
                    s.scale = (s.scale * (1.0 + d.x * 0.005)).clamp(0.02, 50.0);
                } else {
                    s.offset += d * ppp;
                }
            }
        } else if active && v_down && self.paint.mode == BakeMode::Manual && buttons[0] {
            // Hold V: move, Shift rotates, Ctrl scales the buffer.
            let t = &mut self.paint.transform;
            if mods.shift {
                t.rotation += d.x * 0.01;
            } else if mods.command {
                t.scale = (t.scale * (1.0 + d.x * 0.005)).clamp(0.05, 20.0);
            } else {
                t.offset += d * ppp;
            }
        } else if active && mods.shift && !mods.command && !mods.alt && buttons[2] {
            // Shift + right drag rotates the environment.
            self.view.env_rotation += d.x * 0.01;
        } else if active && mods.command && buttons[2] {
            // Ctrl + right drag: horizontal changes size, vertical changes flow.
            let b = &mut self.paint.brush;
            b.size = (b.size + d.x * ppp).clamp(1.0, 2000.0);
            b.flow = (b.flow - d.y * 0.005).clamp(0.0, 1.0);
        } else if response.clicked() && mods.alt && mods.shift {
            if let Some(hp) = hover {
                self.pick_color(to_px(hp), (w, h));
            }
        } else if active && (mods.alt || buttons[1]) {
            // Navigation: Alt + left/middle/right drag; middle drag also pans.
            let moves = (mods.alt && (buttons[0] || buttons[2])) || buttons[1];
            if moves && d != Vec2::ZERO {
                let doc = self.doc.as_mut().unwrap();
                if self.paint.camera_moving(gpu, doc) {
                    if mods.alt && buttons[0] {
                        self.camera.orbit(d);
                    } else if buttons[1] {
                        self.camera.pan(d, rect.height());
                    } else if mods.alt && buttons[2] {
                        self.camera.dolly((1.0 + (d.x - d.y) * -0.005).clamp(0.5, 2.0));
                    }
                } else {
                    self.message = Some((CAMERA_LOCKED.into(), time));
                }
            }
        } else if pressed && response.hovered() && !mods.alt && !v_down && !s_down {
            if let Some(hp) = hover {
                let doc = self.doc.as_mut().unwrap();
                if self.paint.tool == Tool::PaintThrough && stencil.is_none() {
                    self.message = Some(("Paint Through needs a stencil: Window > Stencils > Add.".into(), time));
                } else if let Err(e) = self.paint.begin_stroke(gpu, doc, &self.camera, (w, h), to_px(hp), pressure) {
                    self.message = Some((e, time));
                }
            }
        }
        let frame_cursor = self.bindings.get(crate::shortcuts::Action::FrameCursor);
        if response.hovered() && ui.input(|i| i.key_pressed(frame_cursor.logical_key) && i.modifiers.matches_exact(frame_cursor.modifiers)) {
            if let Some(hp) = hover {
                let doc = self.doc.as_mut().unwrap();
                if let Some(point) = self.paint.pick(gpu, doc, &self.camera, (w, h), to_px(hp)) {
                    let dist = self.camera.distance.min((point - self.camera.eye()).length());
                    self.camera.target = point;
                    self.camera.distance = dist.max(doc.mesh.radius() * 0.05);
                }
            }
        }
        let doc = self.doc.as_mut().unwrap();
        if response.hovered() && scroll != 0.0 {
            if self.paint.camera_moving(gpu, doc) {
                self.camera.dolly(0.999f32.powf(scroll));
            } else {
                self.message = Some((CAMERA_LOCKED.into(), time));
            }
        }
        self.paint.flush(gpu, doc, stencil);
        let doc = &*doc;

        if doc.material_version != self.viewport_material {
            if let Err(e) = self.viewport.set_material(&self.gpu, doc.viewport_code()) {
                log::error!("Material shader failed to compile:
{e}");
                self.error = Some("Material shader failed to compile; see the log.".into());
            }
            self.viewport_material = doc.material_version;
        }
        self.viewport.render(
            &self.gpu,
            &self.camera,
            &doc.mesh,
            &doc.gpu_mesh,
            doc.scene.bind_group.as_ref().unwrap(),
            &self.view,
            w,
            h,
        );

        let view = self.viewport.output_view().unwrap();
        self.paint.draw_overlay(&self.gpu, view, (w, h));
        let layer = doc.stencils.active().map(|s| doc.scene.image_layer(&s.image)).unwrap_or(-1);
        self.stencil_overlay.draw(&self.gpu, view, (w, h), &doc.stencils, &doc.scene.images, layer);
        let tex_id = match self.viewport_texture {
            Some((id, tw, th)) if tw == w && th == h => id,
            Some((id, _, _)) => {
                self.renderer.write().update_egui_texture_from_wgpu_texture(
                    &self.gpu.device,
                    view,
                    wgpu::FilterMode::Nearest,
                    id,
                );
                self.viewport_texture = Some((id, w, h));
                id
            }
            None => {
                let id = self.renderer.write().register_native_texture(
                    &self.gpu.device,
                    view,
                    wgpu::FilterMode::Nearest,
                );
                self.viewport_texture = Some((id, w, h));
                id
            }
        };
        ui.painter().image(
            tex_id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        if self.view.show_tiles {
            self.tile_overlay(ui, rect);
        }
        if self.paint.unbaked {
            let orange = egui::Color32::from_rgb(240, 150, 40);
            ui.painter().rect_stroke(rect.shrink(1.5), 0.0, egui::Stroke::new(3.0, orange), egui::StrokeKind::Inside);
        }
        if let Some(hp) = hover.filter(|p| rect.contains(*p)) {
            if !mods.alt {
                let r = self.paint.brush.size * 0.5 / ppp;
                ui.painter().circle_stroke(hp, r, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(200)));
                ui.painter().circle_stroke(hp, r + 1.0, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(120)));
            }
        }
        if response.dragged() || response.hovered() {
            ui.ctx().request_repaint();
        }
    }

    fn save_smart_material(&mut self, l: crate::ops::LayerRef) {
        // Thumbnail: the current viewport, center-cropped and scaled to 256 x 256.
        let thumb = self.viewport.output_texture().map(|t| {
            let (w, h) = (t.width(), t.height());
            let bytes = self.gpu.read_texture(t, 0, 4);
            let img = image::RgbaImage::from_raw(w, h, bytes).unwrap();
            let side = w.min(h);
            let crop = image::imageops::crop_imm(&img, (w - side) / 2, (h - side) / 2, side, side).to_image();
            (256, image::imageops::resize(&crop, 256, 256, image::imageops::FilterType::Triangle).into_raw())
        });
        let Some(doc) = &self.doc else { return };
        match crate::smartmat::save(&self.gpu, doc, l, thumb) {
            Ok(path) => {
                self.message = Some((format!("Saved smart material {}", path.display()), 0.0));
                self.library.refresh();
            }
            Err(e) => self.error = Some(format!("Could not save smart material: {e:#}")),
        }
    }

    /// Samples the displayed color under the cursor into the brush values.
    fn pick_color(&mut self, px: Vec2, size: (u32, u32)) {
        let Some(tex) = self.viewport.output_texture() else { return };
        let bytes = self.gpu.read_texture(tex, 0, 4);
        let (x, y) = (px.x as u32, px.y as u32);
        if x >= size.0 || y >= size.1 {
            return;
        }
        let i = ((y * size.0 + x) * 4) as usize;
        let lin = |v: u8| {
            let c = v as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let c = [lin(bytes[i]), lin(bytes[i + 1]), lin(bytes[i + 2])];
        let ch = match self.view.display {
            DisplayMode::Channel(c) => c as usize,
            _ => Channel::BaseColor.index(),
        };
        let v = &mut self.paint.values;
        v.values[ch] = if Channel::ALL[ch].components() == 3 { c } else { [bytes[i] as f32 / 255.0; 3] };
        v.enabled[ch] = true;
    }

    fn viewport_state(&self) -> crate::projectfile::ViewportState {
        crate::projectfile::ViewportState {
            camera: Some(self.camera),
            exposure: self.view.exposure,
            view_transform: Some(self.view.view_transform.key().into()),
            env_rotation: self.view.env_rotation,
            environment: self.environment.as_ref().map(|p| p.display().to_string()),
        }
    }

    fn save(&mut self) -> bool {
        let Some(path) = self.doc.as_ref().and_then(|d| d.path.clone()) else { return self.save_as() };
        self.save_to(path)
    }

    fn save_as(&mut self) -> bool {
        let Some(doc) = &self.doc else { return false };
        let stem = doc.mesh.source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
        let default_name = format!("{}.crate", doc.name.clone().unwrap_or_else(|| capitalize(&stem)));
        let Some(path) = rfd::FileDialog::new().set_title("Save Project").set_file_name(default_name).save_file() else {
            return false;
        };
        self.save_to(crate::projectfile::with_extension(path))
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        let vs = self.viewport_state();
        let export = self.export_dialog.settings.clone();
        let Some(doc) = &mut self.doc else { return false };
        match crate::projectfile::save(&self.gpu, doc, &path, &vs, export.as_ref()) {
            Ok(_) => {
                self.message = Some((format!("Saved {}", path.display()), 0.0));
                self.recent = crate::recent::Recent::record(&path);
                true
            }
            Err(e) => {
                self.error = Some(format!("Save failed: {e:#}"));
                false
            }
        }
    }

    fn open_project_dialog(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Open Project (select the .crate folder)").pick_folder() else { return };
        self.open_project(&dir);
    }

    fn open_project(&mut self, dir: &std::path::Path) {
        match crate::projectfile::load(&self.gpu, dir) {
            Ok(loaded) => {
                let project = loaded.project;
                self.paint = PaintEngine::new(&self.gpu);
                self.viewport = Viewport::new(&self.gpu, &project.scene.layout);
                self.viewport.set_mesh(&self.gpu, &project.gpu_mesh);
                self.view = ViewSettings { height_scale: project.channels.height_scale, ..Default::default() };
                self.view.exposure = loaded.viewport.exposure;
                self.view.view_transform =
                    loaded.viewport.view_transform.as_deref().and_then(crate::viewport::ViewTransform::from_key).unwrap_or_default();
                self.view.env_rotation = loaded.viewport.env_rotation;
                self.camera = loaded.viewport.camera.unwrap_or_else(|| {
                    let mut c = Camera::default();
                    c.frame(project.mesh.center(), project.mesh.radius());
                    c
                });
                self.bake_selection = MeshMap::ALL.map(|m| project.maps.status(m) != MapStatus::UpToDate);
                self.export_dialog.settings = loaded.export;
                self.environment = None;
                if let Some(env) = loaded.viewport.environment.map(PathBuf::from) {
                    if self.viewport.set_environment(&self.gpu, Some(&env)).is_ok() {
                        self.view.hdri = true;
                        self.environment = Some(env);
                    }
                }
                self.doc = Some(project);
                self.thumbs.clear();
                self.library.project_changed();
                self.viewport_material = u64::MAX;
                self.error = None;
                self.recent = crate::recent::Recent::record(dir);
            }
            Err(e) => self.error = Some(format!("Could not open {}: {e:#}", dir.display())),
        }
    }

    fn recent_menu(&mut self, ui: &mut egui::Ui) {
        if self.recent.projects.is_empty() {
            ui.add_enabled(false, egui::Button::new("No recent projects"));
        }
        let mut chosen = None;
        for (path, label) in self.recent.projects.iter().zip(self.recent.labels()) {
            let found = path.is_dir();
            let r = ui.add_enabled(found, egui::Button::new(label));
            let r = if found {
                r.on_hover_text(path.display().to_string())
            } else {
                r.on_disabled_hover_text(format!("Not found: {}", path.display()))
            };
            if r.clicked() {
                chosen = Some(path.clone());
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(!self.recent.projects.is_empty(), egui::Button::new("Clear Recent")).clicked() {
            self.recent.clear();
            self.recent.save();
            ui.close();
        }
        if let Some(path) = chosen {
            self.guarded(Pending::OpenRecent(path));
        }
    }

    /// Runs `action` now, or asks first if there are unsaved changes.
    fn guarded(&mut self, action: Pending) {
        if self.doc.as_ref().is_some_and(|d| d.has_unsaved_changes()) {
            self.pending = Some(action);
        } else {
            self.run_pending(action);
        }
    }

    fn run_pending(&mut self, action: Pending) {
        match action {
            Pending::New => self.new_project = Some(NewProjectDialog::new()),
            Pending::Open => self.open_project_dialog(),
            Pending::OpenRecent(path) => self.open_project(&path),
            Pending::Quit => self.allow_close = true,
            Pending::Restart => {
                self.allow_close = true;
                self.restart = true;
            }
        }
    }

    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending.clone() else { return };
        let mut choice = None;
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label("Save the project before continuing?");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        let quitting = matches!(action, Pending::Quit | Pending::Restart);
        match choice {
            Some(0) => {
                self.pending = None;
                if self.save() {
                    self.run_pending(action);
                }
            }
            Some(1) => {
                self.pending = None;
                self.run_pending(action);
            }
            Some(_) => self.pending = None,
            None => {}
        }
        if quitting && self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn side_panels(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = &mut self.doc else { return };
        let mut library_action = None;
        egui::Panel::left("left").resizable(true).default_size(280.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                paint_panels(ui, &self.gpu, doc, &mut self.paint);
                egui::CollapsingHeader::new("Library").default_open(true).show(ui, |ui| {
                    library_action = self.library.ui(ui, &self.gpu, doc);
                });
            });
        });
        use crate::ui_library::LibraryAction;
        match library_action {
            Some(LibraryAction::InsertSmartMaterial(path)) => {
                let above = doc.selection.map(|s| s.layer());
                match crate::smartmat::insert(&self.gpu, doc, &path, above) {
                    Ok(l) => doc.selection = Some(crate::ops::Selection::Layer(l)),
                    Err(e) => self.error = Some(format!("Could not add smart material: {e:#}")),
                }
            }
            Some(LibraryAction::MeshMapMask(m)) => {
                if let Some(l) = doc.selection.map(|s| s.layer()) {
                    let sel = doc.edit(&self.gpu, "Add mesh map mask", |g| crate::ops::add_mask_mesh_map(g, l, m));
                    doc.selection = Some(sel);
                }
            }
            Some(LibraryAction::ShowMeshMap(m)) => self.view.display = DisplayMode::MeshMap(m.index() as u32),
            Some(LibraryAction::UseAlpha(path)) => match doc.import_image(&self.gpu, &path) {
                Ok(name) => self.paint.brush.alpha = name,
                Err(e) => self.error = Some(format!("Could not use alpha: {e:#}")),
            },
            Some(LibraryAction::LoadStencil(path)) => {
                if let Err(e) = doc.add_stencil(&self.gpu, &path) {
                    self.error = Some(format!("Could not load stencil: {e:#}"));
                }
            }
            Some(LibraryAction::LoadEnvironment(path)) => match self.viewport.set_environment(&self.gpu, Some(&path)) {
                Ok(()) => {
                    self.view.hdri = true;
                    self.environment = Some(path);
                }
                Err(e) => self.error = Some(format!("Could not load environment: {e:#}")),
            },
            None => {}
        }
        let mut actions = PanelActions::default();
        egui::Panel::right("right").resizable(true).default_size(330.0).show(ui, |ui| {
            let h = ui.available_height();
            egui::Panel::bottom("properties").resizable(true).default_size(h * 0.5).show(ui, |ui| {
                crate::ui_layer_list::panel_header(ui, "Properties");
                ui_layers::properties_panel(ui, &self.gpu, doc);
            });
            egui::CentralPanel::default().show(ui, |ui| {
                crate::ui_layer_list::panel_header(ui, "Layers");
                self.thumbs.prune(doc);
                crate::ui_layer_list::layers_panel(ui, &self.gpu, doc, &mut actions, &mut self.thumbs);
            });
        });
        if let Some(l) = actions.save_smart_material {
            self.save_smart_material(l);
        }
        let doc = self.doc.as_mut().unwrap();
        if let Some(path) = actions.insert_smart_material.take() {
            let above = doc.selection.map(|s| s.layer());
            match crate::smartmat::insert(&self.gpu, doc, &path, above) {
                Ok(l) => doc.selection = Some(crate::ops::Selection::Layer(l)),
                Err(e) => self.error = Some(format!("Could not add smart material: {e:#}")),
            }
        }
        if actions.edit_as_graph.is_some() {
            if let Some(sel) = doc.selection {
                self.node_editor.open_for(sel.layer());
            }
        }
        if !self.node_editor.open && doc.preview.is_some() {
            doc.set_preview(&self.gpu, None);
        }
        if self.node_editor.open {
            egui::Panel::bottom("node editor").resizable(true).default_size(380.0).min_size(220.0).show(ui, |ui| {
                self.node_editor.ui(ui, &self.gpu, doc);
            });
        }
    }

    /// UV layout with the UDIM grid, drawn in the viewport corner while `U` is on.
    fn tile_overlay(&self, ui: &egui::Ui, rect: egui::Rect) {
        let doc = self.doc.as_ref().unwrap();
        let m = &doc.mesh;
        let max_u = m.tiles.iter().map(|t| (t - 1001) % 10).max().unwrap_or(0) + 1;
        let max_v = m.tiles.iter().map(|t| (t - 1001) / 10).max().unwrap_or(0) + 1;
        let cell = (rect.width() * 0.4 / max_u as f32).min(rect.height() * 0.5 / max_v as f32).min(220.0);
        let size = egui::vec2(cell * max_u as f32, cell * max_v as f32);
        let origin = rect.left_bottom() + egui::vec2(16.0, -16.0);
        let area = egui::Rect::from_min_size(origin - egui::vec2(0.0, size.y), size);
        let p = ui.painter_at(area.expand(2.0));
        p.rect_filled(area, 2.0, egui::Color32::from_black_alpha(170));
        let to_screen = |u: f32, v: f32| egui::pos2(origin.x + u * cell, origin.y - v * cell);
        if m.indices.len() / 3 <= 200_000 {
            let stroke = egui::Stroke::new(0.5, egui::Color32::from_rgba_unmultiplied(200, 200, 200, 90));
            for tri in m.indices.chunks_exact(3) {
                let pts = [0, 1, 2].map(|k| {
                    let uv = m.vertices[tri[k] as usize].uv;
                    to_screen(uv[0], uv[1])
                });
                p.line_segment([pts[0], pts[1]], stroke);
                p.line_segment([pts[1], pts[2]], stroke);
                p.line_segment([pts[2], pts[0]], stroke);
            }
        }
        for v in 0..max_v {
            for u in 0..max_u {
                let udim = 1001 + u + 10 * v;
                let r = egui::Rect::from_two_pos(
                    to_screen(u as f32, v as f32),
                    to_screen(u as f32 + 1.0, v as f32 + 1.0),
                );
                let used = m.tiles.contains(&udim);
                let color = if used { egui::Color32::from_rgb(240, 200, 80) } else { egui::Color32::from_gray(110) };
                p.rect_stroke(r, 0.0, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
                p.text(
                    r.left_top() + egui::vec2(4.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    udim.to_string(),
                    egui::FontId::monospace(12.0),
                    color,
                );
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.new_project_dialog(ctx);
        if self.updater.ui(ctx) {
            self.guarded(Pending::Restart);
            if self.allow_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        if self.show_import_log {
            let mut open = true;
            let diags = self.doc.as_ref().map(|d| &d.mesh.diagnostics).or(self.failed_import_log.as_ref());
            egui::Window::new("Import Log").open(&mut open).show(ctx, |ui| match diags {
                Some(diags) if !diags.is_empty() => {
                    egui::Grid::new("diags").striped(true).show(ui, |ui| {
                        for d in diags {
                            let color = match d.severity {
                                Severity::Error => egui::Color32::from_rgb(230, 90, 80),
                                Severity::Warning => egui::Color32::from_rgb(230, 180, 70),
                                Severity::Info => egui::Color32::from_gray(170),
                            };
                            ui.colored_label(color, format!("{:?}", d.severity));
                            ui.monospace(d.code);
                            ui.label(&d.message);
                            ui.end_row();
                        }
                    });
                }
                Some(_) => {
                    ui.label("No diagnostics.");
                }
                None => {
                    ui.label("No mesh imported yet.");
                }
            });
            self.show_import_log = open;
        }

        if let Some(doc) = &mut self.doc {
            self.export_dialog.show(ctx, &self.gpu, doc, &mut self.evaluator);
        }
        if self.windows.show(ctx, &self.gpu, self.doc.as_mut(), &mut self.prefs, &mut self.view.height_scale) {
            self.bindings = crate::shortcuts::Bindings::new(&self.prefs.shortcuts);
        }

        if self.show_mesh_maps {
            let mut open = true;
            // Settings in a scrolling column on the left, the bake preview filling the rest.
            egui::Window::new("Mesh Maps")
                .open(&mut open)
                .resizable(true)
                .default_size([980.0, 620.0])
                .min_size([640.0, 380.0])
                .show(ctx, |ui| {
                    if self.doc.is_none() {
                        ui.label("No project.");
                        return;
                    }
                    let (width, height) = (ui.available_width() * 0.35, ui.available_height());
                    ui.horizontal_top(|ui| {
                        // Top-down: the column would otherwise inherit the split's left-to-right
                        // layout and put the sections side by side.
                        let top_down = egui::Layout::top_down(egui::Align::Min);
                        ui.allocate_ui_with_layout(egui::vec2(width, height), top_down, |ui| {
                            ui.set_width(width);
                            egui::ScrollArea::vertical().id_salt("mesh maps settings").max_height(height).show(ui, |ui| {
                                ui.with_layout(top_down, |ui| {
                                    ui.set_width(width - 16.0);
                                    self.mesh_maps_ui(ui);
                                });
                            });
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            if let Some(doc) = self.doc.as_mut() {
                                self.bake_pane.ui(ui, &self.gpu, &self.renderer, doc);
                            }
                        });
                    });
                });
            self.show_mesh_maps = open;
        }
    }

    fn new_project_dialog(&mut self, ctx: &egui::Context) {
        let mut create: Option<(PathBuf, ImportSettings)> = None;
        let Some(dlg) = &mut self.new_project else { return };
        let mut open = true;
        egui::Window::new("New Project")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Mesh:");
                    let name = dlg
                        .mesh_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(none)".into());
                    ui.label(name);
                    if ui.button("Select…").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Mesh", &["glb", "gltf", "obj"])
                            .pick_file()
                        {
                            dlg.mesh_path = Some(p);
                        }
                    }
                });
                egui::Grid::new("import settings").num_columns(2).show(ui, |ui| {
                    let s = &mut dlg.settings;
                    ui.label("Tile resolution");
                    egui::ComboBox::from_id_salt("res")
                        .selected_text(s.tile_resolution.to_string())
                        .show_ui(ui, |ui| {
                            for r in [1024, 2048, 4096] {
                                ui.selectable_value(&mut s.tile_resolution, r, r.to_string());
                            }
                        });
                    ui.end_row();
                    ui.label("UV set");
                    ui.add(egui::DragValue::new(&mut s.uv_set).range(0..=7));
                    ui.end_row();
                    ui.label("Compute tangents");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.compute_tangents, TangentMode::Auto, "Auto");
                        ui.selectable_value(&mut s.compute_tangents, TangentMode::Always, "Always");
                    });
                    ui.end_row();
                    ui.label("Scale");
                    ui.add(egui::DragValue::new(&mut s.scale).speed(0.01).range(0.0001..=10000.0));
                    ui.end_row();
                    ui.label("Up axis");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.up_axis, UpAxis::Y, "Y");
                        ui.selectable_value(&mut s.up_axis, UpAxis::Z, "Z");
                    });
                    ui.end_row();
                });
                if let Some(e) = &dlg.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                }
                ui.add_enabled_ui(dlg.mesh_path.is_some(), |ui| {
                    if ui.button("Create").clicked() {
                        create = Some((dlg.mesh_path.clone().unwrap(), dlg.settings.clone()));
                    }
                });
            });
        if !open {
            self.new_project = None;
        }
        if let Some((path, settings)) = create {
            if self.open_mesh(path, settings) {
                self.new_project = None;
                self.save_as();
            } else if let Some(dlg) = &mut self.new_project {
                dlg.error = self.error.clone();
            }
        }
    }

    fn mesh_maps_ui(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = &mut self.doc else {
            ui.label("No project.");
            return;
        };
        let baking = doc.bake_job.is_some();
        egui::Grid::new("maps").num_columns(2).striped(true).show(ui, |ui| {
            for m in MeshMap::ALL {
                let possible = !m.needs_high() || doc.maps.high_poly.is_some();
                if !possible {
                    self.bake_selection[m.index()] = false;
                }
                ui.add_enabled(possible, egui::Checkbox::new(&mut self.bake_selection[m.index()], m.name()))
                    .on_disabled_hover_text("Needs a high-poly mesh (below).");
                let st = doc.maps.status(m);
                let color = match st {
                    MapStatus::UpToDate => egui::Color32::from_rgb(120, 200, 120),
                    MapStatus::OutOfDate => egui::Color32::from_rgb(230, 180, 70),
                    MapStatus::Missing => egui::Color32::from_gray(150),
                };
                ui.colored_label(color, st.label());
                ui.end_row();
            }
        });
        ui.add_enabled_ui(!baking && self.bake_selection.iter().any(|b| *b), |ui| {
            if ui.button("Bake selected").clicked() {
                let maps: Vec<MeshMap> =
                    MeshMap::ALL.into_iter().filter(|m| self.bake_selection[m.index()]).collect();
                doc.start_bake(&self.gpu, &maps);
                self.bake_selection = [false; 8];
            }
        });
        if let Some(job) = &doc.bake_job {
            ui.add(egui::ProgressBar::new(job.progress()).show_percentage());
        }
        if let Some(pct) = doc.maps.last_misses {
            ui.label(format!("Missed: {pct:.1}% of texels"));
        }
        ui.separator();
        egui::CollapsingHeader::new("High poly").default_open(true).show(ui, |ui| {
            let before = (doc.maps.settings, doc.maps.high_poly.clone());
            // The file name only, so a long path doesn't widen the settings; the path on hover.
            match &doc.maps.high_poly {
                Some(p) => {
                    let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
                    ui.add(egui::Label::new(egui::RichText::new(name).monospace()).truncate()).on_hover_text(p.display().to_string());
                }
                None => {
                    ui.label(egui::RichText::new("none").monospace());
                }
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!baking, egui::Button::new("Browse…")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Mesh", &["glb", "gltf", "obj"]).set_title("High-poly mesh").pick_file() {
                        doc.maps.high_poly = Some(p);
                    }
                }
                if ui.add_enabled(!baking && doc.maps.high_poly.is_some(), egui::Button::new("Clear")).clicked() {
                    doc.maps.high_poly = None;
                }
            });
            let s = &mut doc.maps.settings;
            egui::Grid::new("high poly settings").num_columns(2).show(ui, |ui| {
                ui.label("Auto cage");
                ui.checkbox(&mut s.auto_cage, "")
                    .on_hover_text("Measure Frontal and Rear distance per vertex from the high poly");
                ui.end_row();
                ui.label("Frontal distance");
                ui.add_enabled(!s.auto_cage, egui::DragValue::new(&mut s.frontal).speed(0.0005).range(0.0..=1.0));
                ui.end_row();
                ui.label("Rear distance");
                ui.add_enabled(!s.auto_cage, egui::DragValue::new(&mut s.rear).speed(0.0005).range(0.0..=1.0));
                ui.end_row();
                ui.label("Match by name");
                ui.checkbox(&mut s.match_by_name, "");
                ui.end_row();
                ui.label("Apply Normal map");
                ui.checkbox(&mut s.apply_normal, "");
                ui.end_row();
            });
            if (doc.maps.settings, doc.maps.high_poly.clone()) != before {
                if doc.maps.high_poly != before.1 {
                    doc.maps.last_misses = None;
                }
                doc.maps.settings.high_poly = crate::highpoly::stamp(doc.maps.high_poly.as_ref());
                doc.maps.refresh_status();
                for m in MeshMap::ALL {
                    if doc.maps.status(m) == MapStatus::OutOfDate {
                        self.bake_selection[m.index()] = true;
                    }
                }
                // Apply Normal map is shown immediately.
                doc.update_scene(&self.gpu);
            }
        });
        ui.separator();
        egui::CollapsingHeader::new("Bake settings").default_open(true).show(ui, |ui| {
            let before = doc.maps.settings;
            let s = &mut doc.maps.settings;
            egui::Grid::new("bake settings").num_columns(2).show(ui, |ui| {
                ui.label("Resolution");
                let res_text =
                    if s.resolution == 0 { "Tile resolution".to_string() } else { s.resolution.to_string() };
                egui::ComboBox::from_id_salt("map res").selected_text(res_text).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.resolution, 0, "Tile resolution");
                    for r in [512u32, 1024, 2048, 4096] {
                        ui.selectable_value(&mut s.resolution, r, r.to_string());
                    }
                });
                ui.end_row();
                ui.label("Samples");
                ui.add(egui::DragValue::new(&mut s.samples).range(1..=1024));
                ui.end_row();
                ui.label("Max distance");
                ui.add(egui::DragValue::new(&mut s.max_distance).speed(0.005).range(0.001..=1.0));
                ui.end_row();
                ui.label("Spread angle");
                ui.add(egui::DragValue::new(&mut s.spread_angle).speed(1.0).range(1.0..=180.0).suffix("°"));
                ui.end_row();
                ui.label("Ignore backfaces");
                ui.checkbox(&mut s.ignore_backfaces, "");
                ui.end_row();
                ui.label("Curvature radius");
                ui.add(egui::DragValue::new(&mut s.curvature_radius).speed(0.0005).range(0.0001..=0.2));
                ui.end_row();
                ui.label("Padding");
                ui.add(egui::DragValue::new(&mut s.padding).range(0..=64).suffix(" texels"));
                ui.end_row();
            });
            if doc.maps.settings != before {
                doc.maps.refresh_status();
                for m in MeshMap::ALL {
                    if doc.maps.status(m) == MapStatus::OutOfDate {
                        self.bake_selection[m.index()] = true;
                    }
                }
            }
        });
    }
}

const CAMERA_LOCKED: &str = "Camera locked: bake or clear the buffer first (Ctrl+B / Ctrl+Delete).";

fn paint_panels(ui: &mut egui::Ui, gpu: &Gpu, doc: &mut Project, paint: &mut PaintEngine) {
    crate::ui_paint::tool_bar(ui, paint);
    ui.separator();
    egui::CollapsingHeader::new("Brush").default_open(true).show(ui, |ui| {
        crate::ui_paint::brush_panel(ui, gpu, doc, paint);
    });
    egui::CollapsingHeader::new("Brush Values").default_open(true).show(ui, |ui| {
        crate::ui_paint::brush_values_panel(ui, doc, paint);
    });
    egui::CollapsingHeader::new("Paint Buffer").default_open(true).show(ui, |ui| {
        crate::ui_paint::paint_buffer_panel(ui, gpu, doc, paint);
    });
    egui::CollapsingHeader::new("Stencils").default_open(true).show(ui, |ui| {
        crate::ui_paint::stencils_panel(ui, gpu, doc);
    });
}

/// Name of the layer or mask layer painting writes to.
fn paint_target_label(doc: &Project) -> String {
    use crate::ops::{self, Selection};
    let Some(sel) = doc.selection else { return "No paint target".into() };
    if ops::paint_target(&doc.graph, sel).is_none() {
        return "No paint target".into();
    }
    match sel {
        Selection::Layer(l) => ops::slot(&doc.graph, l).map(|s| s.name.clone()),
        Selection::MaskLayer(l, i) => ops::slot(&doc.graph, l)
            .zip(ops::mask_slot(&doc.graph, l, i))
            .map(|(s, m)| format!("{} > {}", s.name, m.name)),
    }
    .unwrap_or_else(|| "No paint target".into())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Neutral dark theme: flat grey panels, small corner radii, blue selection.
fn apply_theme(ctx: &egui::Context) {
    use egui::{Color32, CornerRadius, Stroke};
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = Color32::from_rgb(40, 40, 43);
        v.window_fill = Color32::from_rgb(44, 44, 47);
        v.extreme_bg_color = Color32::from_rgb(28, 28, 30);
        v.faint_bg_color = Color32::from_rgb(46, 46, 49);
        v.window_stroke = Stroke::new(1.0, Color32::from_gray(25));
        v.selection.bg_fill = Color32::from_rgb(56, 74, 100);
        v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(200, 215, 235));
        v.hyperlink_color = Color32::from_rgb(110, 160, 230);
        let r = CornerRadius::same(2);
        v.window_corner_radius = CornerRadius::same(3);
        v.menu_corner_radius = r;
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = r;
        }
        v.widgets.noninteractive.bg_fill = Color32::from_rgb(40, 40, 43);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(30));
        v.widgets.inactive.bg_fill = Color32::from_rgb(58, 58, 62);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(52, 52, 56);
        v.widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 75);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(64, 64, 68);
        v.widgets.active.bg_fill = Color32::from_rgb(56, 74, 100);
        v.slider_trailing_fill = true;
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
    });
}
