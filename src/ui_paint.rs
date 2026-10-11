//! Brush, Brush Values and Paint Buffer panels.
//! See docs/reference/brushes-tools-stencils.md and paint-buffer.md.

use egui::RichText;

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::paint::{BakeMode, OnCameraMove, PaintEngine, SizeSpace, Tool};
use crate::project::Project;
use crate::ui_theme as t;
use crate::ui_widgets::{self as w, Check, Field, Section, Toggle};

pub fn tool_bar(ui: &mut egui::Ui, paint: &mut PaintEngine) {
    w::segmented(ui, &mut paint.tool, &[(Tool::Paint, "Paint"), (Tool::Erase, "Erase"), (Tool::PaintThrough, "Paint Through")])
        .on_hover_text("B, E, P");
}

pub fn brush_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, paint: &mut PaintEngine) {
    let b = &mut paint.brush;
    ui.add(Field::new(&mut b.size, 1.0..=2000.0).label("Size").decimals(0).speed(0.5).no_fill());
    w::setting(ui, "Size space", |ui| {
        w::segmented(ui, &mut b.size_space, &[(SizeSpace::Screen, "Screen"), (SizeSpace::World, "World")]);
    });
    ui.add(Field::new(&mut b.hardness, 0.0..=1.0).label("Hardness").decimals(2));
    ui.add(Field::new(&mut b.flow, 0.0..=1.0).label("Flow").decimals(2));
    ui.add(Field::new(&mut b.opacity, 0.0..=1.0).label("Opacity").decimals(2));
    ui.add(Field::new(&mut b.spacing, 1.0..=200.0).label("Spacing").decimals(0).suffix(" %").speed(0.5).no_fill());
    w::setting(ui, "Alpha", |ui| {
        let label = if b.alpha.is_empty() { "Round".to_string() } else { b.alpha.clone() };
        if ui.add(w::secondary(label)).clicked() {
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
        if !b.alpha.is_empty() && ui.add(w::ghost("×")).on_hover_text("Round brush").clicked() {
            b.alpha.clear();
        }
    });
    ui.add(Field::new(&mut b.angle, 0.0..=360.0).label("Angle").decimals(0).suffix("°"));
    w::setting(ui, "Follow stroke", |ui| ui.add(Toggle(&mut b.follow_stroke)));
    Section::new("Jitter").id_salt("brush jitter").default_open(false).show(ui, |ui| {
        ui.add(Field::new(&mut b.jitter_size, 0.0..=1.0).label("Size").decimals(2));
        ui.add(Field::new(&mut b.jitter_angle, 0.0..=1.0).label("Angle").decimals(2));
        ui.add(Field::new(&mut b.jitter_position, 0.0..=1.0).label("Position").decimals(2));
        ui.add(Field::new(&mut b.jitter_flow, 0.0..=1.0).label("Flow").decimals(2));
    });
    Section::new("Pressure").id_salt("brush pressure").default_open(false).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(Check::new(&mut b.pressure_size, "Size"));
            ui.add(Check::new(&mut b.pressure_flow, "Flow"));
            ui.add(Check::new(&mut b.pressure_opacity, "Opacity"));
        });
    });
}

/// Summary for the Brush section header: the alpha's name, or Round.
pub fn brush_summary(paint: &PaintEngine) -> String {
    if paint.brush.alpha.is_empty() { "Round".into() } else { paint.brush.alpha.clone() }
}

pub fn brush_values_panel(ui: &mut egui::Ui, p: &Project, paint: &mut PaintEngine) {
    let v = &mut paint.values;
    for c in Channel::ALL {
        if !p.channels.is_enabled(c) {
            continue;
        }
        let i = c.index();
        ui.horizontal(|ui| {
            ui.add(Check::new(&mut v.enabled[i], ""));
            ui.add_enabled_ui(v.enabled[i], |ui| match c {
                Channel::BaseColor | Channel::Emissive => {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(96.0, t::H_CONTROL), egui::Sense::hover());
                        ui.painter().text(r.left_center() + egui::vec2(9.0, 0.0), egui::Align2::LEFT_CENTER, c.name(), t::body(), t::TEXT);
                        let mut rgb = v.values[i];
                        let size = egui::vec2(ui.available_width(), t::H_CONTROL - 4.0);
                        ui.spacing_mut().interact_size = size;
                        if ui.color_edit_button_rgb(&mut rgb).changed() {
                            v.values[i] = rgb;
                        }
                    });
                }
                Channel::Normal => {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(60.0, t::H_CONTROL), egui::Sense::hover());
                        ui.painter().text(r.left_center() + egui::vec2(9.0, 0.0), egui::Align2::LEFT_CENTER, c.name(), t::body(), t::TEXT);
                        let fw = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(40.0);
                        for k in 0..3 {
                            ui.add(Field::new(&mut v.values[i][k], 0.0..=1.0).decimals(2).width(fw));
                        }
                    });
                }
                Channel::Height => {
                    ui.add(Field::new(&mut v.values[i][0], -1.0..=1.0).label(c.name()));
                }
                _ => {
                    ui.add(Field::new(&mut v.values[i][0], 0.0..=1.0).label(c.name()));
                }
            });
        });
    }
    ui.horizontal(|ui| {
        ui.add_space(14.0 + ui.spacing().item_spacing.x);
        ui.add(Field::new(&mut v.mask_value, 0.0..=1.0).label("Mask value")).on_hover_text("Value painted into masks");
    });
}

pub fn paint_buffer_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, paint: &mut PaintEngine) {
    w::setting(ui, "Bake mode", |ui| {
        let mut mode = paint.mode;
        w::segmented(ui, &mut mode, &[(BakeMode::Live, "Live"), (BakeMode::Manual, "Manual")]);
        if mode != paint.mode {
            paint.set_mode(gpu, p, mode);
        }
    });
    w::setting(ui, "Resolution scale", |ui| {
        let mut scale = paint.buffer_scale;
        w::segmented(ui, &mut scale, &[(0.5f32, "0.5×"), (1.0, "1×"), (2.0, "2×")]);
        if scale != paint.buffer_scale {
            // Changing the resolution loses unbaked paint; bake it first.
            let _ = paint.bake(gpu, p);
            paint.buffer_scale = scale;
        }
    });
    ui.add_enabled_ui(paint.mode == BakeMode::Manual, |ui| {
        w::setting(ui, "On camera move", |ui| {
            egui::ComboBox::from_id_salt("on camera move")
                .width(ui.available_width())
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
    });
    w::setting(ui, "Show buffer", |ui| ui.add(Toggle(&mut paint.show_buffer)));
    ui.horizontal(|ui| {
        ui.add_enabled_ui(paint.unbaked, |ui| {
            if ui.add(w::secondary("Bake")).on_hover_text("Ctrl+B").clicked() {
                if let Err(e) = paint.bake(gpu, p) {
                    log::warn!("{e}");
                }
            }
            if ui.add(w::secondary("Clear")).on_hover_text("Ctrl+Delete").clicked() {
                paint.clear_buffer(gpu);
            }
        });
        if ui.add_enabled(!paint.transform.is_identity(), w::secondary("Reset transform")).clicked() {
            paint.transform = Default::default();
        }
    });
    if paint.unbaked {
        ui.label(RichText::new("Unbaked paint in the buffer").color(t::ATTENTION));
    }
    Section::new("Projection").id_salt("paint projection").default_open(false).show(ui, |ui| {
        let pr = &mut paint.projection;
        w::setting(ui, "Backface culling", |ui| ui.add(Toggle(&mut pr.backface_culling)));
        ui.add(Field::new(&mut pr.falloff_start, 0.0..=90.0).label("Falloff start").decimals(0).suffix("°"));
        ui.add(Field::new(&mut pr.falloff_end, 0.0..=90.0).label("Falloff end").decimals(0).suffix("°"));
        if pr.falloff_end < pr.falloff_start {
            pr.falloff_end = pr.falloff_start;
        }
        ui.add(Field::new(&mut pr.depth_tolerance, 0.0..=0.1).label("Depth tolerance").decimals(4).speed(0.0001).no_fill());
        w::setting(ui, "Project through", |ui| ui.add(Toggle(&mut pr.project_through)));
    });
}

pub fn stencils_panel(ui: &mut egui::Ui, p: &mut Project) {
    for i in 0..p.stencils.stencils.len() {
        let name = p.stencils.stencils[i].image.clone();
        let active = p.stencils.active == Some(i);
        if ui.selectable_label(active, RichText::new(name).font(t::value())).on_hover_text("Active stencil (Alt+S cycles)").clicked() {
            p.stencils.active = Some(i);
        }
    }
    let enabled: Vec<bool> = Channel::ALL.map(|c| p.channels.is_enabled(c)).to_vec();
    let Some(s) = p.stencils.active_mut() else {
        empty_state(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            w::hint(ui, "No stencils. Hold");
            w::keycap(ui, "S");
            w::hint(ui, "and drag to move a stencil.");
        });
        return;
    };
    ui.add(Field::new(&mut s.opacity, 0.0..=1.0).label("Opacity (display)").decimals(2));
    ui.horizontal_wrapped(|ui| {
        ui.add(Check::new(&mut s.visible, "Visible"));
        ui.add(Check::new(&mut s.use_alpha, "Use alpha"));
        ui.add(Check::new(&mut s.tile, "Tile"));
        ui.add(Check::new(&mut s.flip, "Flip"));
    });
    w::hint(ui, "Paint Through target");
    ui.horizontal_wrapped(|ui| {
        for c in Channel::ALL {
            if enabled[c.index()] {
                ui.add(Check::new(&mut s.targets[c.index()], c.name()));
            }
        }
    });
    if ui.add(w::secondary("Reset transform")).on_hover_text("S, then R").clicked() {
        s.reset_transform();
    }
}

/// The Stencils section's header actions: add a stencil, remove the active one.
pub fn stencils_actions(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
    if let Some(a) = p.stencils.active {
        if ui.add(w::ghost("−")).on_hover_text("Remove the active stencil").clicked() {
            p.stencils.remove(a);
        }
    }
    let full = p.stencils.stencils.len() >= crate::stencil::MAX_STENCILS;
    if ui.add_enabled(!full, w::ghost("+")).on_hover_text("Add a stencil").clicked() {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Image", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"])
            .pick_file()
        {
            if let Err(e) = p.add_stencil(gpu, &path) {
                log::error!("{e:#}");
            }
        }
    }
}

/// An empty state: the instruction inside a dashed box.
pub fn empty_state(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::NONE.inner_margin(egui::Margin::same(12)).corner_radius(t::R_CONTROL);
    let resp = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(content);
    });
    let r = resp.response.rect;
    let stroke = egui::Stroke::new(1.0, t::BORDER_STRONG);
    let dash = egui::Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], stroke, 4.0, 3.0);
    ui.painter().extend(dash);
}
