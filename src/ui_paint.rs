//! Brush, Brush Values and Paint Buffer panels.
//! See docs/reference/brushes-tools-stencils.md and paint-buffer.md.

use egui::RichText;

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::paint::{BakeMode, OnCameraMove, PaintEngine, SizeSpace, Tool};
use crate::project::Project;

pub fn tool_bar(ui: &mut egui::Ui, paint: &mut PaintEngine) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut paint.tool, Tool::Paint, "Paint").on_hover_text("B");
        ui.selectable_value(&mut paint.tool, Tool::Erase, "Erase").on_hover_text("E");
        ui.selectable_value(&mut paint.tool, Tool::PaintThrough, "Paint Through").on_hover_text("P");
    });
}

pub fn brush_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, paint: &mut PaintEngine) {
    let b = &mut paint.brush;
    egui::Grid::new("brush").num_columns(2).show(ui, |ui| {
        ui.label("Size");
        ui.add(egui::DragValue::new(&mut b.size).range(1.0..=2000.0).speed(0.5));
        ui.end_row();
        ui.label("Size space");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut b.size_space, SizeSpace::Screen, "Screen");
            ui.selectable_value(&mut b.size_space, SizeSpace::World, "World");
        });
        ui.end_row();
        ui.label("Hardness");
        ui.add(egui::Slider::new(&mut b.hardness, 0.0..=1.0));
        ui.end_row();
        ui.label("Flow");
        ui.add(egui::Slider::new(&mut b.flow, 0.0..=1.0));
        ui.end_row();
        ui.label("Opacity");
        ui.add(egui::Slider::new(&mut b.opacity, 0.0..=1.0));
        ui.end_row();
        ui.label("Spacing");
        ui.add(egui::DragValue::new(&mut b.spacing).range(1.0..=200.0).suffix(" %"));
        ui.end_row();
        ui.label("Alpha");
        ui.horizontal(|ui| {
            let label = if b.alpha.is_empty() { "Round".to_string() } else { b.alpha.clone() };
            if ui.button(label).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"])
                    .pick_file()
                {
                    match p.import_image(gpu, &path) {
                        Ok(name) => b.alpha = name,
                        Err(e) => log::error!("{e:#}"),
                    }
                }
            }
            if !b.alpha.is_empty() && ui.small_button("×").clicked() {
                b.alpha.clear();
            }
        });
        ui.end_row();
        ui.label("Angle");
        ui.add(egui::DragValue::new(&mut b.angle).range(0.0..=360.0).suffix("°"));
        ui.end_row();
        ui.label("Follow stroke");
        ui.checkbox(&mut b.follow_stroke, "");
        ui.end_row();
    });
    egui::CollapsingHeader::new("Jitter").show(ui, |ui| {
        ui.add(egui::Slider::new(&mut b.jitter_size, 0.0..=1.0).text("Size"));
        ui.add(egui::Slider::new(&mut b.jitter_angle, 0.0..=1.0).text("Angle"));
        ui.add(egui::Slider::new(&mut b.jitter_position, 0.0..=1.0).text("Position"));
        ui.add(egui::Slider::new(&mut b.jitter_flow, 0.0..=1.0).text("Flow"));
    });
    egui::CollapsingHeader::new("Pressure").show(ui, |ui| {
        ui.checkbox(&mut b.pressure_size, "Size");
        ui.checkbox(&mut b.pressure_flow, "Flow");
        ui.checkbox(&mut b.pressure_opacity, "Opacity");
    });
}

pub fn brush_values_panel(ui: &mut egui::Ui, p: &Project, paint: &mut PaintEngine) {
    let v = &mut paint.values;
    egui::Grid::new("brush values").num_columns(2).show(ui, |ui| {
        for c in Channel::ALL {
            if !p.channels.is_enabled(c) {
                continue;
            }
            let i = c.index();
            ui.checkbox(&mut v.enabled[i], c.name());
            ui.add_enabled_ui(v.enabled[i], |ui| match c {
                Channel::BaseColor | Channel::Emissive => {
                    ui.color_edit_button_rgb(&mut v.values[i]);
                }
                Channel::Normal => {
                    ui.horizontal(|ui| {
                        for k in 0..3 {
                            ui.add(egui::DragValue::new(&mut v.values[i][k]).range(0.0..=1.0).speed(0.01));
                        }
                    });
                }
                Channel::Height => {
                    ui.add(egui::Slider::new(&mut v.values[i][0], -1.0..=1.0));
                }
                _ => {
                    ui.add(egui::Slider::new(&mut v.values[i][0], 0.0..=1.0));
                }
            });
            ui.end_row();
        }
        ui.label("Mask value");
        ui.add(egui::Slider::new(&mut v.mask_value, 0.0..=1.0)).on_hover_text("Value painted into masks");
        ui.end_row();
    });
}

pub fn paint_buffer_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, paint: &mut PaintEngine) {
    egui::Grid::new("paint buffer").num_columns(2).show(ui, |ui| {
        ui.label("Bake mode");
        ui.horizontal(|ui| {
            let mut mode = paint.mode;
            ui.selectable_value(&mut mode, BakeMode::Live, "Live");
            ui.selectable_value(&mut mode, BakeMode::Manual, "Manual");
            if mode != paint.mode {
                paint.set_mode(gpu, p, mode);
            }
        });
        ui.end_row();
        ui.label("Resolution scale");
        ui.horizontal(|ui| {
            for s in [0.5f32, 1.0, 2.0] {
                if ui.selectable_label(paint.buffer_scale == s, format!("{s}×")).clicked() && paint.buffer_scale != s {
                    // Changing the resolution loses unbaked paint; bake it first.
                    let _ = paint.bake(gpu, p);
                    paint.buffer_scale = s;
                }
            }
        });
        ui.end_row();
        ui.label("On camera move");
        ui.add_enabled_ui(paint.mode == BakeMode::Manual, |ui| {
            egui::ComboBox::from_id_salt("on camera move")
                .selected_text(match paint.on_camera_move {
                    OnCameraMove::Bake => "Bake",
                    OnCameraMove::Clear => "Clear",
                    OnCameraMove::LockCamera => "Lock camera",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut paint.on_camera_move, OnCameraMove::Bake, "Bake");
                    ui.selectable_value(&mut paint.on_camera_move, OnCameraMove::Clear, "Clear");
                    ui.selectable_value(&mut paint.on_camera_move, OnCameraMove::LockCamera, "Lock camera");
                });
        });
        ui.end_row();
        ui.label("Show buffer");
        ui.checkbox(&mut paint.show_buffer, "");
        ui.end_row();
    });
    ui.horizontal(|ui| {
        ui.add_enabled_ui(paint.unbaked, |ui| {
            if ui.button("Bake").on_hover_text("Ctrl+B").clicked() {
                if let Err(e) = paint.bake(gpu, p) {
                    log::warn!("{e}");
                }
            }
            if ui.button("Clear").on_hover_text("Ctrl+Delete").clicked() {
                paint.clear_buffer(gpu);
            }
        });
        if ui.add_enabled(!paint.transform.is_identity(), egui::Button::new("Reset transform")).clicked() {
            paint.transform = Default::default();
        }
    });
    egui::CollapsingHeader::new("Projection").show(ui, |ui| {
        let pr = &mut paint.projection;
        ui.checkbox(&mut pr.backface_culling, "Backface culling");
        ui.add(egui::Slider::new(&mut pr.falloff_start, 0.0..=90.0).text("Falloff start").suffix("°"));
        ui.add(egui::Slider::new(&mut pr.falloff_end, 0.0..=90.0).text("Falloff end").suffix("°"));
        if pr.falloff_end < pr.falloff_start {
            pr.falloff_end = pr.falloff_start;
        }
        ui.add(egui::DragValue::new(&mut pr.depth_tolerance).range(0.0..=0.1).speed(0.0001).prefix("Depth tolerance "));
        ui.checkbox(&mut pr.project_through, "Project through");
    });
    if paint.unbaked {
        ui.label(RichText::new("Unbaked paint in the buffer").color(egui::Color32::from_rgb(240, 150, 40)));
    }
}

pub fn stencils_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
    ui.horizontal(|ui| {
        let full = p.stencils.stencils.len() >= crate::stencil::MAX_STENCILS;
        if ui.add_enabled(!full, egui::Button::new("Add")).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"])
                .pick_file()
            {
                if let Err(e) = p.add_stencil(gpu, &path) {
                    log::error!("{e:#}");
                }
            }
        }
        if let Some(a) = p.stencils.active {
            if ui.button("Remove").clicked() {
                p.stencils.remove(a);
            }
        }
    });
    let mut remove = None;
    for i in 0..p.stencils.stencils.len() {
        let name = p.stencils.stencils[i].image.clone();
        ui.horizontal(|ui| {
            if ui.radio(p.stencils.active == Some(i), name).on_hover_text("Active stencil (Alt+S cycles)").clicked() {
                p.stencils.active = Some(i);
            }
        });
        let _ = &mut remove;
    }
    if let Some(i) = remove {
        p.stencils.remove(i);
    }
    let enabled: Vec<bool> = Channel::ALL.map(|c| p.channels.is_enabled(c)).to_vec();
    let Some(s) = p.stencils.active_mut() else {
        ui.label(RichText::new("No stencils. Hold S and drag to move a stencil.").weak());
        return;
    };
    ui.add(egui::Slider::new(&mut s.opacity, 0.0..=1.0).text("Opacity (display)"));
    ui.horizontal(|ui| {
        ui.checkbox(&mut s.visible, "Visible");
        ui.checkbox(&mut s.use_alpha, "Use alpha");
        ui.checkbox(&mut s.tile, "Tile");
        ui.checkbox(&mut s.flip, "Flip");
    });
    ui.label("Paint Through target");
    ui.horizontal_wrapped(|ui| {
        for c in Channel::ALL {
            if enabled[c.index()] {
                ui.checkbox(&mut s.targets[c.index()], c.name());
            }
        }
    });
    if ui.button("Reset transform").on_hover_text("S, then R").clicked() {
        s.reset_transform();
    }
}
