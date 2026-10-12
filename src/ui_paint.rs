//! The tool rail, the viewport toolbar, and the Brush, Brush Values and Paint Buffer panels.
//! See docs/reference/brushes-tools-stencils.md, paint-buffer.md and windows-and-panels.md#viewport.

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::icons::{self, Icon};
use crate::meshmaps::{MapStatus, MeshMap};
use crate::paint::{BakeMode, Brush, OnCameraMove, PaintEngine, SizeSpace, Tool};
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
        let tools = [
            (Tool::Paint, "Paint", Action::ToolPaint),
            (Tool::Erase, "Erase", Action::ToolErase),
            (Tool::PaintThrough, "Paint Through", Action::ToolPaintThrough),
            (Tool::GeoFill, "Geo Fill", Action::ToolGeoFill),
            (Tool::Curve, "Curve", Action::ToolCurve),
        ];
        for (tool, name, action) in tools {
            let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
            let on = paint.tool == tool;
            let p = ui.painter();
            if on {
                p.rect(rect, t::R_CONTROL, t::accent(0.14), Stroke::new(1.0, t::accent(0.45)), StrokeKind::Inside);
            } else if resp.hovered() {
                p.rect_filled(rect, t::R_CONTROL, t::BG_INPUT);
            }
            // Blender's colored tool icons: full color when current or hovered, dimmed otherwise.
            let color = if on || resp.hovered() { Color32::WHITE } else { t::TEXT_2 };
            let icon = match tool {
                Tool::Paint => Icon::Brush,
                Tool::Erase => Icon::Erase,
                Tool::PaintThrough => Icon::Stencil,
                Tool::GeoFill => Icon::GeoFill,
                Tool::Curve => Icon::Curve,
            };
            icons::paint(p, icon, rect.shrink(8.0), color);
            if resp.on_hover_text(format!("{name}  {}", format(&bindings.get(action)))).clicked() {
                paint.tool = tool;
            }
        }
        ui.add_space(4.0);
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, t::DIVIDER));
    });
}

/// Resolution of the tip preview drawn inside the brush cursor.
const TIP_PREVIEW_RES: u32 = 128;

/// The brush tip as shown inside the cursor, made again when the tip changes.
#[derive(Default)]
pub struct TipPreview {
    /// Alpha image name and layer, and hardness (round tips only).
    key: Option<(String, i32, u32)>,
    tex: Option<egui::TextureHandle>,
}

impl TipPreview {
    /// The tip in white, its alpha the dab's strength: the round falloff or the alpha image.
    fn texture(&mut self, ctx: &egui::Context, gpu: &Gpu, p: &Project, brush: &Brush) -> Option<&egui::TextureHandle> {
        let layer = p.scene.image_layer(&brush.alpha);
        let hardness = if layer >= 0 { 0 } else { (brush.hardness.clamp(0.0, 0.99) * 1000.0) as u32 };
        let key = (brush.alpha.clone(), layer, hardness);
        if self.key.as_ref() != Some(&key) {
            let n = TIP_PREVIEW_RES;
            let alpha: Vec<u8> = if layer >= 0 {
                // Read back from the image pool: what the dab shader samples.
                let res = p.scene.images.res;
                let bytes = gpu.read_texture(&p.scene.images.texture, layer as u32, 4);
                let img = image::RgbaImage::from_raw(res, res, bytes)?;
                let small = image::imageops::resize(&img, n, n, image::imageops::FilterType::Triangle);
                small.pixels().map(|px| {
                    let [r, g, b, a] = px.0.map(|c| c as f32 / 255.0);
                    ((0.2126 * r + 0.7152 * g + 0.0722 * b) * a * 255.0) as u8
                }).collect()
            } else {
                // The dab shader's falloff: full strength out to the hardness, then smoothstep.
                let h = hardness as f32 / 1000.0;
                (0..n * n).map(|i| {
                    let (x, y) = ((i % n) as f32 + 0.5, (i / n) as f32 + 0.5);
                    let d = ((x / n as f32 * 2.0 - 1.0).powi(2) + (y / n as f32 * 2.0 - 1.0).powi(2)).sqrt();
                    let t = ((d - h) / (1.0 - h)).clamp(0.0, 1.0);
                    let v = if d >= 1.0 { 0.0 } else { 1.0 - t * t * (3.0 - 2.0 * t) };
                    (v * 255.0) as u8
                }).collect()
            };
            let rgba: Vec<u8> = alpha.iter().flat_map(|&a| [255, 255, 255, a]).collect();
            let img = egui::ColorImage::from_rgba_unmultiplied([n as usize, n as usize], &rgba);
            self.tex = Some(ctx.load_texture("brush tip preview", img, egui::TextureOptions::LINEAR));
            self.key = Some(key);
        }
        self.tex.as_ref()
    }
}

/// The brush cursor at `center`: the tip, rotated to the brush angle, inside a circle of the
/// brush size. `radius` is in points.
pub fn brush_cursor(ui: &egui::Ui, gpu: &Gpu, p: &Project, brush: &Brush, preview: &mut TipPreview, center: egui::Pos2, radius: f32) {
    let painter = ui.painter();
    if let Some(tex) = preview.texture(ui.ctx(), gpu, p, brush) {
        // The alpha image fills the square around the circle, as in the dab shader.
        let rect = egui::Rect::from_center_size(center, vec2(radius, radius) * 2.0);
        egui::Image::new(tex)
            .rotate(brush.angle.to_radians(), vec2(0.5, 0.5))
            .tint(Color32::from_white_alpha(70))
            .paint_at(ui, rect);
    }
    painter.circle_stroke(center, radius, Stroke::new(1.0, Color32::from_white_alpha(200)));
    painter.circle_stroke(center, radius + 1.0, Stroke::new(1.0, Color32::from_black_alpha(120)));
}

/// The strip above the viewport: Size, Flow and Hardness, and Bake Mesh Maps on the right.
/// `mesh_maps` is whether the Mesh Maps window is open.
pub fn viewport_toolbar(ui: &mut egui::Ui, p: &Project, paint: &mut PaintEngine, mesh_maps: &mut bool, layout: &mut crate::camera::Layout) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, t::BG_PANEL);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
    let inner = rect.shrink2(vec2(t::PANEL_PAD, 0.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    ui.spacing_mut().item_spacing.x = 6.0;
    if paint.tool == Tool::GeoFill {
        // Geo Fill uses none of the three: what a click fills instead
        // (docs/reference/brushes-tools-stencils.md#geo-fill).
        use crate::geo::{FillBy, SelectMode};
        ui.label(RichText::new("Select").color(t::TEXT_2));
        let select = SelectMode::ALL.map(|s| {
            let icon = match s {
                SelectMode::Click => Icon::SelectClick,
                SelectMode::Brush => Icon::SelectCircle,
                SelectMode::Box => Icon::SelectBox,
            };
            (s, icon, s.name())
        });
        w::segmented_icons(&mut ui, &mut paint.select, &select);
        ui.add_space(8.0);
        ui.label(RichText::new("Fill by").color(t::TEXT_2));
        let fill_by = FillBy::ALL.map(|f| {
            let icon = match f {
                FillBy::Face => Icon::Triangle,
                FillBy::Island => Icon::Island,
                FillBy::Part => Icon::Part,
                FillBy::Material => Icon::Material,
                FillBy::Tile => Icon::Grid,
            };
            (f, icon, f.name())
        });
        w::segmented_icons(&mut ui, &mut paint.fill_by, &fill_by);
        // The Brush circle's size, as for painting.
        if paint.select == SelectMode::Brush {
            ui.add_space(8.0);
            ui.add(Field::new(&mut paint.brush.size, 1.0..=2000.0).label("Size").decimals(0).suffix(" px").speed(0.5).no_fill().width(128.0));
        }
    } else {
        let b = &mut paint.brush;
        ui.add(Field::new(&mut b.size, 1.0..=2000.0).label("Size").decimals(0).suffix(" px").speed(0.5).no_fill().width(128.0));
        let mut flow = b.flow * 100.0;
        if ui.add(Field::new(&mut flow, 0.0..=100.0).label("Flow").decimals(0).suffix("%").width(128.0)).changed() {
            b.flow = flow / 100.0;
        }
        ui.add(Field::new(&mut b.hardness, 0.0..=1.0).label("Hardness").decimals(2).width(128.0));
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // Maps that need a high poly don't count without one: they can't be baked.
        let possible = |m: &MeshMap| !m.needs_high() || p.maps.high_poly.is_some();
        let outdated = MeshMap::ALL.into_iter().filter(|m| possible(m) && p.maps.status(*m) != MapStatus::UpToDate).count();
        let hint = match outdated {
            0 => "Open the Mesh Maps window: all maps are baked".to_string(),
            n => format!("Open the Mesh Maps window: {n} maps not baked or out of date"),
        };
        if bake_button(ui, *mesh_maps, outdated > 0).on_hover_text(hint).clicked() {
            *mesh_maps = !*mesh_maps;
        }
        // Right to left: the layout control sits left of Bake Mesh Maps (docs/reference/uv-view.md).
        use crate::camera::Layout;
        let options = [(Layout::ThreeD, "3D"), (Layout::Split, "Split"), (Layout::Uv, "UV")];
        crate::ui_widgets::segmented_width(ui, layout, &options, 150.0).on_hover_text("3D (F2), Split (F1) or UV view (F3)");
    });
}

/// "Bake Mesh Maps" with Blender's UV project icon; highlighted while the window is open, with
/// an attention dot while some map isn't baked or is out of date.
fn bake_button(ui: &mut egui::Ui, open: bool, attention: bool) -> egui::Response {
    let label = ui.painter().layout_no_wrap("Bake Mesh Maps".into(), t::body_strong(), t::TEXT);
    let dot = if attention { 12.0 } else { 0.0 };
    let width = 8.0 + 16.0 + 6.0 + label.size().x + dot + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, t::H_CONTROL), Sense::click());
    let p = ui.painter();
    let bg = if open || resp.hovered() { t::BG_ACTIVE } else { t::BG_INPUT };
    let border = if open { t::accent(0.45) } else { t::BORDER };
    p.rect(rect, t::R_CONTROL, bg, Stroke::new(1.0, border), StrokeKind::Inside);
    let y = rect.center().y;
    let icon = egui::Rect::from_center_size(pos2(rect.left() + 16.0, y), vec2(16.0, 16.0));
    icons::paint(p, Icon::Bake, icon, if open || resp.hovered() { t::TEXT } else { t::TEXT_2 });
    let x = rect.left() + 30.0;
    let lw = label.size().x;
    p.galley(pos2(x, y - label.size().y / 2.0), label, t::TEXT);
    if attention {
        p.circle_filled(pos2(x + lw + 9.0, y), 3.0, t::ATTENTION);
    }
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
        if !b.alpha.is_empty() && ui.add(w::ghost_icon(Icon::Close)).on_hover_text("Round brush").clicked() {
            b.alpha.clear();
        }
    });
    ui.add(Field::new(&mut b.angle, 0.0..=360.0).label("Angle").decimals(0).suffix("°"));
    w::setting(ui, "Follow stroke", |ui| ui.add(Toggle(&mut b.follow_stroke)));
    // UV view only: whole texels (docs/reference/2d-paint.md#pixel-brush).
    w::setting(ui, "Pixel", |ui| ui.add(Toggle(&mut b.pixel)).on_hover_text("In the UV view: a hard square of whole texels"));
    if b.pixel {
        ui.add(Field::new(&mut b.pixel_size, 1..=64).label("Pixel size").suffix(" texels").speed(0.1).no_fill());
    }
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

/// One row per enabled channel, laid out like a Fill layer's Values: the name in the label
/// column and the value filling the rest, with a check in front that turns the channel on.
pub fn brush_values_panel(ui: &mut egui::Ui, p: &Project, paint: &mut PaintEngine) {
    let v = &mut paint.values;
    for c in Channel::ALL {
        if !p.channels.is_enabled(c) {
            continue;
        }
        let i = c.index();
        ui.horizontal(|ui| {
            ui.add(Check::new(&mut v.enabled[i], "")).on_hover_text(format!("Paint {}", c.name()));
            ui.add_enabled_ui(v.enabled[i], |ui| {
                w::setting(ui, c.name(), |ui| match c {
                    Channel::BaseColor | Channel::Emissive => {
                        // The color swatch takes the interact size: give it the whole column.
                        ui.spacing_mut().interact_size = vec2(ui.available_width(), t::H_CONTROL);
                        w::color_edit(ui, &mut v.values[i]);
                    }
                    Channel::Normal => {
                        let fw = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(40.0);
                        for x in &mut v.values[i] {
                            ui.add(Field::new(x, 0.0..=1.0).decimals(2).width(fw));
                        }
                    }
                    Channel::Height => {
                        ui.add(Field::new(&mut v.values[i][0], -1.0..=1.0));
                    }
                    _ => {
                        ui.add(Field::new(&mut v.values[i][0], 0.0..=1.0));
                    }
                });
            });
        });
    }
    ui.horizontal(|ui| {
        // Lines up with the rows above, past their checks.
        ui.add_space(14.0 + ui.spacing().item_spacing.x);
        w::setting(ui, "Mask value", |ui| ui.add(Field::new(&mut v.mask_value, 0.0..=1.0))).on_hover_text("Value painted into masks");
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
        if ui.add(w::ghost_icon(Icon::Remove)).on_hover_text("Remove the active stencil").clicked() {
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
