//! File > Export Textures dialog and the export preset editor. See Painter-docs/reference/export.md.

use egui::RichText;

use crate::export::{self, ExportSettings, FileFormat, NormalFormat, Padding, Preset};
use crate::gpu::Gpu;
use crate::project::Project;

pub struct ExportDialog {
    pub open: bool,
    pub settings: Option<ExportSettings>,
    presets: Vec<Preset>,
    /// Preset being edited (Presets > New).
    editing: Option<Preset>,
    result: Option<Result<export::Report, String>>,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self { open: false, settings: None, presets: export::all_presets(), editing: None, result: None }
    }
}

impl ExportDialog {
    pub fn show(&mut self, ctx: &egui::Context, gpu: &Gpu, p: &mut Project, eval: &mut export::UvEvaluator) {
        if !self.open {
            return;
        }
        let s = self.settings.get_or_insert_with(|| {
            let dir = p.mesh.source.parent().map(|d| d.join("export")).unwrap_or_else(|| "export".into());
            ExportSettings::new(dir)
        });
        let mut open = true;
        let mut run = false;
        egui::Window::new("Export Textures").open(&mut open).resizable(false).show(ctx, |ui| {
            egui::Grid::new("export").num_columns(2).show(ui, |ui| {
                ui.label("Output directory");
                ui.horizontal(|ui| {
                    ui.label(s.output_dir.display().to_string());
                    if ui.button("…").clicked() {
                        if let Some(d) = rfd::FileDialog::new().set_directory(&s.output_dir).pick_folder() {
                            s.output_dir = d;
                        }
                    }
                });
                ui.end_row();
                ui.label("Preset");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("preset").selected_text(&s.preset).show_ui(ui, |ui| {
                        for pr in &self.presets {
                            ui.selectable_value(&mut s.preset, pr.name.clone(), &pr.name);
                        }
                    });
                    if ui.button("New…").on_hover_text("New preset from the selected one").clicked() {
                        let mut base = self.presets.iter().find(|x| x.name == s.preset).cloned().unwrap_or_else(|| export::builtin_presets()[0].clone());
                        base.name = format!("{} copy", base.name);
                        base.raw = false;
                        self.editing = Some(base);
                    }
                });
                ui.end_row();
                ui.label("File name template");
                ui.text_edit_singleline(&mut s.template);
                ui.end_row();
                ui.label("Format");
                let fmt_text = match s.format {
                    None => "Per preset".to_string(),
                    Some((f, b)) => format!("{} {}", f.ext().to_uppercase(), if f == FileFormat::Exr { format!("{b}f") } else { b.to_string() }),
                };
                egui::ComboBox::from_id_salt("format").selected_text(fmt_text).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.format, None, "Per preset");
                    for f in [FileFormat::Png, FileFormat::Tga, FileFormat::Exr] {
                        for &b in f.depths() {
                            let label = format!("{} {}", f.ext().to_uppercase(), if f == FileFormat::Exr { format!("{b}f") } else { b.to_string() });
                            ui.selectable_value(&mut s.format, Some((f, b)), label);
                        }
                    }
                });
                ui.end_row();
                ui.label("Resolution");
                let res = p.tile_resolution();
                let label = if s.resolution == 0 { format!("{res} (tile)") } else { s.resolution.to_string() };
                egui::ComboBox::from_id_salt("res").selected_text(label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.resolution, 0, format!("{res} (tile)"));
                    let mut r = res / 2;
                    while r >= 256 {
                        ui.selectable_value(&mut s.resolution, r, r.to_string());
                        r /= 2;
                    }
                });
                ui.end_row();
                ui.label("Padding");
                ui.horizontal(|ui| {
                    let mut kind = match s.padding {
                        Padding::None => 0,
                        Padding::Texels(_) => 1,
                        Padding::Infinite => 2,
                    };
                    ui.selectable_value(&mut kind, 0, "None");
                    ui.selectable_value(&mut kind, 1, "Texels");
                    ui.selectable_value(&mut kind, 2, "Infinite");
                    let mut n = if let Padding::Texels(n) = s.padding { n } else { 8 };
                    if kind == 1 {
                        ui.add(egui::DragValue::new(&mut n).range(1..=256));
                    }
                    s.padding = match kind {
                        0 => Padding::None,
                        1 => Padding::Texels(n),
                        _ => Padding::Infinite,
                    };
                });
                ui.end_row();
                ui.label("Normal format");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.normal_format, NormalFormat::OpenGl, "OpenGL");
                    ui.selectable_value(&mut s.normal_format, NormalFormat::DirectX, "DirectX");
                });
                ui.end_row();
                ui.label("Tiles");
                ui.horizontal_wrapped(|ui| {
                    for &t in &p.mesh.tiles {
                        let mut on = s.tiles.is_empty() || s.tiles.contains(&t);
                        if ui.checkbox(&mut on, t.to_string()).changed() {
                            if s.tiles.is_empty() {
                                s.tiles = p.mesh.tiles.clone();
                            }
                            s.tiles.retain(|x| *x != t);
                            if on {
                                s.tiles.push(t);
                            }
                            if s.tiles.len() == p.mesh.tiles.len() {
                                s.tiles.clear();
                            }
                        }
                    }
                });
                ui.end_row();
            });
            ui.separator();
            if ui.button(RichText::new("Export").strong()).clicked() {
                run = true;
            }
            match &self.result {
                Some(Ok(r)) => {
                    ui.label(format!("Exported {} files.", r.files.len()));
                    for w in &r.warnings {
                        ui.colored_label(egui::Color32::from_rgb(230, 180, 70), w);
                    }
                    if ui.button("Show in file browser").clicked() {
                        crate::prefs::open_folder(&s.output_dir);
                    }
                }
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                }
                None => {}
            }
        });
        if run {
            let s = self.settings.clone().unwrap();
            self.result = Some(export::export(gpu, p, eval, &s).map_err(|e| format!("{e:#}")));
            if let Some(Ok(_)) = &self.result {
                crate::prefs::open_folder(&s.output_dir);
            }
        }
        self.open = open;
        self.preset_editor(ctx);
    }

    fn preset_editor(&mut self, ctx: &egui::Context) {
        let Some(pr) = &mut self.editing else { return };
        let mut open = true;
        let mut save = false;
        egui::Window::new("Export Preset").open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut pr.name);
            });
            ui.label(RichText::new("Sources: base_color.r, roughness, normal.x, ao, 1-roughness, 0, 1 (empty = none)").weak());
            let mut remove = None;
            egui::Grid::new("preset maps").num_columns(8).striped(true).show(ui, |ui| {
                for h in ["Map", "R", "G", "B", "A", "Format", "Bits", ""] {
                    ui.strong(h);
                }
                ui.end_row();
                for (i, m) in pr.maps.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(90.0));
                    for k in 0..4 {
                        ui.add(egui::TextEdit::singleline(&mut m.sources[k]).desired_width(90.0));
                    }
                    egui::ComboBox::from_id_salt(("fmt", i)).width(60.0).selected_text(m.format.ext()).show_ui(ui, |ui| {
                        for f in [FileFormat::Png, FileFormat::Tga, FileFormat::Exr] {
                            ui.selectable_value(&mut m.format, f, f.ext());
                        }
                    });
                    if !m.format.depths().contains(&m.bits) {
                        m.bits = m.format.depths()[0];
                    }
                    egui::ComboBox::from_id_salt(("bits", i)).width(40.0).selected_text(m.bits.to_string()).show_ui(ui, |ui| {
                        for &b in m.format.depths() {
                            ui.selectable_value(&mut m.bits, b, b.to_string());
                        }
                    });
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                pr.maps.remove(i);
            }
            if ui.button("+ Map").clicked() {
                pr.maps.push(export::OutputMap {
                    name: "Map".into(),
                    sources: Default::default(),
                    format: FileFormat::Png,
                    bits: 8,
                    normal_format: None,
                });
            }
            if ui.button("Save").clicked() {
                save = true;
            }
        });
        if save {
            if let Some(pr) = self.editing.take() {
                match export::save_preset(&pr) {
                    Ok(_) => {
                        self.presets = export::all_presets();
                        if let Some(s) = &mut self.settings {
                            s.preset = pr.name.clone();
                        }
                    }
                    Err(e) => log::error!("{e:#}"),
                }
            }
        } else if !open {
            self.editing = None;
        }
    }
}
