//! The tool rail, the viewport toolbar, and the Brush, Brush Values and Paint Buffer panels.
//! See docs/reference/brushes-tools-stencils.md, paint-buffer.md and windows-and-panels.md#viewport.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::paint::{BakeMode, OnCameraMove, PaintEngine, SizeSpace, Tool};
use crate::project::Project;
use crate::ui_theme as t;
use crate::ui_widgets::{self as w, Check, Field, Section, Toggle};

/// Width of the tool rail at the left edge of the window.
const RAIL_W: f32 = 48.0;

/// The tool rail: one icon per tool, the current one in the selection color.
pub fn tool_rail(ui: &mut egui::Ui, paint: &mut PaintEngine, bindings: &crate::shortcuts::Bindings) {
    use crate::shortcuts::{Action, format};
    let frame = egui::Frame::new().fill(t::BG_CHROME).inner_margin(egui::Margin::symmetric(8, 8));
    egui::Panel::left("tool rail").exact_size(RAIL_W).resizable(false).frame(frame).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let tools = [(Tool::Paint, "Paint", Action::ToolPaint), (Tool::Erase, "Erase", Action::ToolErase), (Tool::PaintThrough, "Paint Through", Action::ToolPaintThrough)];
        for (tool, name, action) in tools {
            let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
            let on = paint.tool == tool;
            let p = ui.painter();
            if on {
                p.rect(rect, t::R_CONTROL, t::accent(0.14), Stroke::new(1.0, t::accent(0.45)), StrokeKind::Inside);
            } else if resp.hovered() {
                p.rect_filled(rect, t::R_CONTROL, t::BG_INPUT);
            }
            let color = if on { t::ACCENT } else if resp.hovered() { t::TEXT } else { t::TEXT_2 };
            tool_icon(p, tool, rect.shrink(8.0), color);
            if resp.on_hover_text(format!("{name}  {}", format(&bindings.get(action)))).clicked() {
                paint.tool = tool;
            }
        }
        ui.add_space(4.0);
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, t::DIVIDER));
    });
}

/// Line icons for the tools, drawn into `r`.
fn tool_icon(p: &egui::Painter, tool: Tool, r: Rect, color: Color32) {
    let st = Stroke::new(1.4, color);
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    match tool {
        Tool::Paint => {
            // Handle, ferrule and a rounded tip, bottom left to top right.
            p.line_segment([at(0.95, 0.05), at(0.45, 0.55)], st);
            p.add(egui::Shape::closed_line(vec![at(0.42, 0.45), at(0.55, 0.58), at(0.35, 0.9), at(0.05, 0.95), at(0.1, 0.65)], st));
        }
        Tool::Erase => {
            // A tilted block, its lower half the rubber, on a base line.
            let pts = [at(0.55, 0.05), at(0.95, 0.45), at(0.5, 0.9), at(0.1, 0.5)];
            p.add(egui::Shape::closed_line(pts.to_vec(), st));
            p.line_segment([at(0.33, 0.28), at(0.73, 0.68)], st);
            p.line_segment([at(0.5, 0.95), at(0.95, 0.95)], st);
        }
        Tool::PaintThrough => {
            // Stacked sheets: paint taken through a stencil.
            let sheet = |y: f32| -> Vec<Pos2> { vec![at(0.5, y), at(0.95, y + 0.22), at(0.5, y + 0.44), at(0.05, y + 0.22)] };
            p.add(egui::Shape::closed_line(sheet(0.05), st));
            p.add(egui::Shape::line(vec![at(0.05, 0.5), at(0.5, 0.72), at(0.95, 0.5)], st));
            p.add(egui::Shape::line(vec![at(0.05, 0.72), at(0.5, 0.94), at(0.95, 0.72)], st));
        }
    }
}

/// The strip above the viewport: Size, Flow and Hardness, and the bake mode on the right.
pub fn viewport_toolbar(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, paint: &mut PaintEngine, bake_shortcut: &str) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, t::BG_PANEL);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
    let inner = rect.shrink2(vec2(t::PANEL_PAD, 0.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    ui.spacing_mut().item_spacing.x = 6.0;
    let b = &mut paint.brush;
    ui.add(Field::new(&mut b.size, 1.0..=2000.0).label("Size").decimals(0).suffix(" px").speed(0.5).no_fill().width(128.0));
    let mut flow = b.flow * 100.0;
    if ui.add(Field::new(&mut flow, 0.0..=100.0).label("Flow").decimals(0).suffix("%").width(128.0)).changed() {
        b.flow = flow / 100.0;
    }
    ui.add(Field::new(&mut b.hardness, 0.0..=1.0).label("Hardness").decimals(2).width(128.0));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let live = paint.mode == BakeMode::Live;
        if bake_mode_pill(ui, live).on_hover_text(format!("Switch to {} ({bake_shortcut})", if live { "Manual" } else { "Live" })).clicked() {
            paint.set_mode(gpu, p, if live { BakeMode::Manual } else { BakeMode::Live });
        }
    });
}

/// "● Bake Live": the bake mode, with a green dot while Live.
fn bake_mode_pill(ui: &mut egui::Ui, live: bool) -> egui::Response {
    let font = t::body();
    let label = ui.painter().layout_no_wrap("Bake".into(), font.clone(), t::TEXT_2);
    let mode = ui.painter().layout_no_wrap(if live { "Live" } else { "Manual" }.into(), t::body_strong(), t::TEXT);
    let width = 10.0 + 6.0 + 6.0 + label.size().x + 5.0 + mode.size().x + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, t::H_CONTROL), Sense::click());
    let p = ui.painter();
    let bg = if resp.hovered() { t::BG_ACTIVE } else { t::BG_INPUT };
    p.rect(rect, t::R_CONTROL, bg, Stroke::new(1.0, t::BORDER), StrokeKind::Inside);
    let y = rect.center().y;
    p.circle_filled(pos2(rect.left() + 13.0, y), 3.0, if live { t::SOCK_MATERIAL } else { t::TEXT_3 });
    let x = rect.left() + 22.0;
    let lw = label.size().x;
    p.galley(pos2(x, y - label.size().y / 2.0), label, t::TEXT_2);
    p.galley(pos2(x + lw + 5.0, y - mode.size().y / 2.0), mode, t::TEXT);
    resp
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
