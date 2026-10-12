//! The Layers panel list: rows with content and mask thumbnails, a type icon, blend mode and
//! opacity columns for a chosen channel, nested effects (mask layers), folders and
//! drag-to-reorder. See docs/reference/layers.md and docs/explanation/about-the-interface.md.

use egui::{Color32, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::graph::{BlendMode, MaskSource, NodeId};
use crate::icons::{self, Icon};
use crate::ops::{self, LayerKind, LayerRef, Selection};
use crate::project::Project;
use crate::thumbs::Thumbnails;
use crate::ui_layers::{self, PanelActions};
use crate::ui_theme as t;

const ROW_H: f32 = 48.0;
const EFFECT_H: f32 = 26.0;
const THUMB: f32 = 32.0;
const INDENT: f32 = 14.0;
/// Width of the visibility column.
const EYE_W: f32 = 32.0;
/// Widths of the blend mode and opacity columns.
const BLEND_W: f32 = 38.0;
const OPACITY_W: f32 = 30.0;

const BG: Color32 = t::BG_PANEL;
const ROW: Color32 = t::BG_PANEL;
const ROW_HOVER: Color32 = Color32::from_rgb(0x1c, 0x1d, 0x21);
const EFFECT: Color32 = t::BG_PANEL;
const TEXT: Color32 = t::TEXT;
const TEXT_DIM: Color32 = t::TEXT_3;
const ACCENT: Color32 = t::ACCENT;
const DROP: Color32 = t::ACCENT;
/// Opacity of hidden layers' rows.
const HIDDEN_OPACITY: f32 = 0.55;

/// Selected rows: accent fill with an accent outline.
fn paint_selected(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, t::accent(0.16));
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, t::accent(0.45)), StrokeKind::Inside);
}

/// The blend mode and opacity columns at the right of a row.
fn paint_columns(painter: &egui::Painter, rect: Rect, mode: &str, normal: bool, opacity: &str, dim: bool) {
    let y = rect.center().y;
    let blend_x = rect.right() - OPACITY_W - BLEND_W;
    let mode_color = if !normal { t::ATTENTION } else { TEXT_DIM };
    let font = FontId::monospace(10.5);
    painter.text(pos2(blend_x, y), egui::Align2::LEFT_CENTER, mode, font.clone(), mode_color);
    painter.text(pos2(rect.right() - 10.0, y), egui::Align2::RIGHT_CENTER, opacity, font, if dim { TEXT_DIM } else { t::TEXT_2 });
}

fn short_mode(m: BlendMode) -> &'static str {
    match m {
        BlendMode::Normal => "Norm",
        BlendMode::Multiply => "Mul",
        BlendMode::Screen => "Scrn",
        BlendMode::Overlay => "Ovr",
        BlendMode::Add => "Add",
        BlendMode::Subtract => "Sub",
        BlendMode::Lighten => "Ligh",
        BlendMode::Darken => "Dark",
        BlendMode::ColorDodge => "Dodg",
        BlendMode::ColorBurn => "Burn",
        BlendMode::Max => "Max",
        BlendMode::Min => "Min",
        BlendMode::Combine => "Comb",
    }
}

fn to_srgb8(c: [f32; 3]) -> Color32 {
    egui::Rgba::from_rgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)).into()
}

/// Channel whose blend mode and opacity the rows show (UI state).
fn view_channel(ui: &egui::Ui) -> Channel {
    ui.ctx().data(|d| d.get_temp::<usize>(egui::Id::new("layer view channel"))).map(|i| Channel::ALL[i]).unwrap_or(Channel::BaseColor)
}

/// Strip header of the right panels ("LAYERS", "PROPERTIES"), with `right` drawn right-aligned
/// at its other end.
pub fn panel_header(ui: &mut egui::Ui, title: &str, right: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t::H_SECTION), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
    let text = egui::RichText::new(title.to_uppercase()).font(t::section()).color(t::TEXT_2).extra_letter_spacing(0.6);
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
    let title_right = rect.left() + t::PANEL_PAD + galley.size().x;
    ui.painter().galley(pos2(rect.left() + t::PANEL_PAD, rect.center().y - galley.size().y / 2.0), galley, t::TEXT_2);
    let right_rect = Rect::from_min_max(pos2(title_right + 12.0, rect.top()), pos2(rect.right() - t::PANEL_PAD, rect.bottom()));
    let mut child = ui.new_child(
        egui::UiBuilder::new().id_salt(("panel header", title)).max_rect(right_rect).layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.set_clip_rect(right_rect);
    right(&mut child);
}

/// The Layers header's channel menu: the channel whose blend mode and opacity the rows show.
pub fn channel_menu(ui: &mut egui::Ui, p: &Project) {
    let mut ch = view_channel(ui);
    egui::ComboBox::from_id_salt("layer view channel combo")
        .width(108.0)
        .selected_text(ch.name())
        .show_ui(ui, |ui| {
            for c in Channel::ALL {
                if p.channels.is_enabled(c) {
                    ui.selectable_value(&mut ch, c, c.name());
                }
            }
        })
        .response
        .on_hover_text("Channel whose blend mode and opacity the layers show");
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("layer view channel"), ch.index()));
}

/// Column titles above the rows: NAME, BLEND, OP.
fn column_header(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let p = ui.painter();
    let font = FontId::new(10.0, egui::FontFamily::Name(t::MEDIUM.into()));
    let y = rect.center().y;
    p.text(pos2(rect.left() + EYE_W + 22.0, y), egui::Align2::LEFT_CENTER, "NAME", font.clone(), TEXT_DIM);
    p.text(pos2(rect.right() - OPACITY_W - BLEND_W, y), egui::Align2::LEFT_CENTER, "BLEND", font.clone(), TEXT_DIM);
    p.text(pos2(rect.right() - 10.0, y), egui::Align2::RIGHT_CENTER, "OP", font, TEXT_DIM);
    p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
}

pub fn layers_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, actions: &mut PanelActions, thumbs: &mut Thumbnails) {
    ui.add_space(4.0);
    toolbar(ui, gpu, p);
    ui.add_space(4.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, t::DIVIDER));
    column_header(ui);
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0.0, BG);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let root = p.graph.root;
        if p.graph.node(root).slots.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("No layers").color(TEXT_DIM));
                ui.label(egui::RichText::new("Add a paint or fill layer from the toolbar above.").small().color(TEXT_DIM));
            });
        }
        stack_rows(ui, gpu, p, root, 0, actions, thumbs);
        // Space below the last row accepts library drops too.
        let rest = ui.available_rect_before_wrap();
        if rest.height() > 0.0 {
            let r = ui.allocate_rect(rest, Sense::hover());
            if let Some(path) = crate::ui_widgets::dnd_release::<std::path::PathBuf>(&r) {
                actions.insert_smart_material = Some((*path).clone());
            }
        }
    });
}

// ------------------------------------------------------------------ toolbar

fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), if enabled { Sense::click() } else { Sense::hover() });
    let color = if !enabled {
        Color32::from_rgb(0x44, 0x47, 0x4e)
    } else if resp.hovered() {
        t::TEXT
    } else {
        t::TEXT_2
    };
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, t::R_CONTROL, t::BG_INPUT);
    }
    icons::paint(ui.painter(), icon, rect.shrink(4.0), color);
    resp.on_hover_text(tip)
}

fn toolbar(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
    let sel = p.selection.map(|s| s.layer());
    ui.horizontal(|ui| {
        ui.add_space(t::PANEL_PAD - 4.0);
        ui.spacing_mut().item_spacing.x = 1.0;
        if icon_button(ui, Icon::Brush, "Add paint layer (Ctrl+Shift+N)", true).clicked() {
            ui_layers::add_paint(gpu, p);
        }
        let r = icon_button(ui, Icon::Brush, "Add 2D paint layer: paints every texel from the UV view", true);
        two_d_badge(ui.painter(), r.rect.shrink(4.0));
        if r.clicked() {
            let above = sel;
            let l = p.edit(gpu, "New 2D paint layer", |g| ops::add_paint_2d_layer(g, above));
            p.selection = Some(Selection::Layer(l));
        }
        if icon_button(ui, Icon::Fill, "Add fill layer (Ctrl+Shift+F)", true).clicked() {
            ui_layers::add_fill(gpu, p);
        }
        if icon_button(ui, Icon::Folder, "Add folder (Ctrl+G groups the selection)", true).clicked() {
            let above = sel;
            let l = p.edit(gpu, "Add folder", |g| ops::add_folder(g, above));
            p.selection = Some(Selection::Layer(l));
        }
        ui.add_space(6.0);
        let mask_resp = icon_button(ui, Icon::Mask, "Add mask", sel.is_some());
        egui::Popup::menu(&mask_resp).show(|ui| {
            if let Some(l) = sel {
                if ui.button("White mask").clicked() {
                    p.edit(gpu, "Add mask", |g| ops::add_mask(g, l, false));
                }
                if ui.button("Black mask").clicked() {
                    p.edit(gpu, "Add black mask", |g| ops::add_mask(g, l, true));
                }
            }
        });
        let fx_resp = icon_button(ui, Icon::Effect, "Add effect to the mask (generator, paint, fill, levels)", sel.is_some());
        egui::Popup::menu(&fx_resp).show(|ui| {
            if let Some(l) = sel {
                ui_layers::mask_add_menu(ui, gpu, p, l);
            }
        });
        ui.add_space(6.0);
        if icon_button(ui, Icon::Duplicate, "Duplicate (Ctrl+D)", sel.is_some()).clicked() {
            ui_layers::duplicate_selected(gpu, p);
        }
        if icon_button(ui, Icon::Trash, "Delete (Delete)", sel.is_some()).clicked() {
            ui_layers::delete_selected(gpu, p);
        }
    });
}

// ------------------------------------------------------------------ rows

fn stack_rows(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, stack: NodeId, depth: usize, actions: &mut PanelActions, thumbs: &mut Thumbnails) {
    let count = p.graph.get(stack).map(|n| n.slots.len()).unwrap_or(0);
    for index in (0..count).rev() {
        let l = LayerRef { stack, index };
        let Some(slot) = ops::slot(&p.graph, l).cloned() else { continue };
        layer_row(ui, gpu, p, l, depth, actions, thumbs);
        if slot.expanded {
            if let Some(mask) = slot.mask {
                effect_rows(ui, gpu, p, l, mask, depth);
            }
            if ops::layer_kind(&p.graph, &slot) == LayerKind::Group {
                if let Some(c) = slot.content {
                    stack_rows(ui, gpu, p, c, depth + 1, actions, thumbs);
                }
            }
        }
    }
}

/// A small "2D" in the lower right of an icon: 2D paint (docs/reference/2d-paint.md).
fn two_d_badge(painter: &egui::Painter, icon: Rect) {
    let at = icon.right_bottom() + vec2(3.0, 2.0);
    let font = egui::FontId::new(8.0, egui::FontFamily::Name(t::SEMIBOLD.into()));
    painter.text(at, egui::Align2::RIGHT_BOTTOM, "2D", font, t::ACCENT);
}

/// Paints a content thumbnail for a layer.
fn content_thumb(painter: &egui::Painter, p: &Project, kind: LayerKind, content: Option<NodeId>, r: Rect) {
    painter.rect(r, 4.0, t::BG_APP, Stroke::new(1.0, t::BORDER_STRONG), StrokeKind::Inside);
    match kind {
        LayerKind::Fill => {
            let n = content.and_then(|c| p.graph.get(c));
            let on = |name: &str| n.is_some_and(|n| n.param(name).as_bool());
            let image = n.is_some_and(|n| n.param("base_color_source").as_str() == "Image");
            if let Some(n) = n.filter(|_| on("base_color_on") && !image) {
                painter.rect_filled(r.shrink(1.0), 2.0, to_srgb8(n.param("base_color").as_vec3()));
                // Lower-right triangle: roughness, so material changes are visible at a glance.
                if on("roughness_on") {
                    let g = (n.param("roughness").as_f32().clamp(0.0, 1.0) * 255.0) as u8;
                    let pts = vec![r.right_bottom() - vec2(1.0, 1.0), r.right_bottom() - vec2(r.width() * 0.4, 1.0), r.right_bottom() - vec2(1.0, r.height() * 0.4)];
                    painter.add(egui::Shape::convex_polygon(pts, Color32::from_gray(g), Stroke::NONE));
                }
            } else if image {
                // Hatched: an image fill.
                for i in 0..6 {
                    let t = i as f32 / 6.0;
                    painter.line_segment(
                        [pos2(r.left() + t * r.width(), r.bottom()), pos2(r.right(), r.top() + t * r.height())],
                        Stroke::new(1.0, Color32::from_gray(90)),
                    );
                }
            } else {
                icons::paint(painter, Icon::Fill, r.shrink(5.0), t::TEXT_2);
            }
        }
        LayerKind::Paint | LayerKind::Paint2d => {
            // Checker background with a brush mark.
            let k = r.width() / 4.0;
            for y in 0..4 {
                for x in 0..4 {
                    if (x + y) % 2 == 0 {
                        let cell = Rect::from_min_size(r.min + vec2(x as f32 * k, y as f32 * k), vec2(k, k));
                        painter.rect_filled(cell, 0.0, Color32::from_gray(36));
                    }
                }
            }
            icons::paint(painter, Icon::Brush, r.shrink(5.0), t::TEXT);
        }
        LayerKind::Group => icons::paint(painter, Icon::Folder, r.shrink(5.0), t::TEXT_2),
        LayerKind::Graph => {
            // A node graph: a material socket and a color socket.
            let c = r.center();
            painter.circle_filled(c - vec2(3.0, 0.0), 2.0, t::SOCK_MATERIAL);
            painter.circle_filled(c + vec2(3.0, 0.0), 2.0, t::SOCK_COLOR);
        }
        LayerKind::SmartMaterial => icons::paint(painter, Icon::Material, r.shrink(5.0), t::BRAND),
    }
}

/// Paints a mask thumbnail: the base fill value with a mark for what sits on top.
fn mask_thumb(painter: &egui::Painter, p: &Project, mask: NodeId, r: Rect) {
    let Some(ms) = p.graph.get(mask) else { return };
    let base = ms.mask_slots.first().and_then(|s| match s.source {
        MaskSource::Node(n) => p.graph.get(n).filter(|n| n.ty == "value").map(|n| n.value()),
        _ => None,
    });
    let g = (base.unwrap_or(0.0).clamp(0.0, 1.0) * 255.0) as u8;
    painter.rect_filled(r, 2.0, Color32::from_gray(g));
    let top = ms.mask_slots.iter().rev().find(|s| s.visible);
    let mark = Color32::from_gray(if g > 128 { 30 } else { 230 });
    if let Some(top) = top {
        let icon = match &top.source {
            MaskSource::Levels { .. } => Some(Icon::Levels),
            MaskSource::Node(n) => match p.graph.get(*n).map(|n| n.ty.as_str()) {
                Some(ty) if crate::graph::is_paint_mask(ty) => Some(Icon::Brush),
                Some("value") => None,
                Some(_) => Some(Icon::Generator),
                None => None,
            },
        };
        if let Some(icon) = icon {
            icons::paint(painter, icon, r.shrink(9.0), mark);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn layer_row(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef, depth: usize, actions: &mut PanelActions, thumbs: &mut Thumbnails) {
    let Some(slot) = ops::slot(&p.graph, l).cloned() else { return };
    let kind = ops::layer_kind(&p.graph, &slot);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click_and_drag());
    let content_selected = p.selection == Some(Selection::Layer(l)) || p.multi.contains(&l);
    let mask_selected = matches!(p.selection, Some(Selection::MaskLayer(sl, _)) if sl == l);
    let selected = content_selected || mask_selected;
    let mut painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, if resp.hovered() { ROW_HOVER } else { ROW });
    if selected {
        paint_selected(&painter, rect);
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
    painter.vline(rect.left() + EYE_W - 0.5, rect.y_range(), Stroke::new(1.0, t::DIVIDER));
    if depth > 0 {
        // Folder contents: a guide line on the left.
        let x = rect.left() + EYE_W + 4.0 + (depth as f32 - 0.5) * INDENT;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, t::BORDER_STRONG));
    }

    // Visibility.
    let eye_rect = Rect::from_center_size(pos2(rect.left() + EYE_W / 2.0, rect.center().y), vec2(18.0, 18.0));
    let eye = ui.interact(eye_rect, ui.id().with(("eye", l.stack, l.index)), Sense::click());
    let (eye_icon, eye_color) = if slot.visible { (Icon::Visible, t::TEXT_2) } else { (Icon::Hidden, Color32::from_rgb(0x4a, 0x4d, 0x55)) };
    icons::paint(&painter, eye_icon, eye_rect.shrink(2.0), eye_color);
    if !slot.visible {
        painter.set_opacity(HIDDEN_OPACITY);
    }
    if eye.clicked() {
        p.edit(gpu, "Toggle visibility", |g| {
            if let Some(s) = ops::slot_mut(g, l) {
                s.visible = !s.visible;
            }
        });
    }

    let mut x = rect.left() + EYE_W + 6.0 + depth as f32 * INDENT;
    // Expand arrow: folders and layers with effects.
    let expandable = kind == LayerKind::Group || slot.mask.is_some();
    let arrow_rect = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(14.0, 18.0));
    if expandable {
        let arrow = ui.interact(arrow_rect, ui.id().with(("arrow", l.stack, l.index)), Sense::click());
        let c = arrow_rect.center();
        let pts = if slot.expanded {
            vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
        } else {
            vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
        };
        painter.add(egui::Shape::convex_polygon(pts, if arrow.hovered() { TEXT } else { TEXT_DIM }, Stroke::NONE));
        if arrow.clicked() {
            if let Some(s) = ops::slot_mut(&mut p.graph, l) {
                s.expanded = !s.expanded;
            }
        }
    }
    x += 14.0;

    // Content thumbnail.
    let content_rect = Rect::from_min_size(pos2(x, rect.center().y - THUMB / 2.0), vec2(THUMB, THUMB));
    let paint_thumb = match (kind, slot.content) {
        (LayerKind::Paint | LayerKind::Paint2d, Some(c)) => thumbs.get(ui.ctx(), gpu, p, c),
        _ => None,
    };
    match paint_thumb {
        Some(tex) => {
            painter.image(tex, content_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => content_thumb(&painter, p, kind, slot.content, content_rect),
    }
    if content_selected {
        painter.rect_stroke(content_rect.expand(1.0), 4.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
    }
    x += THUMB + 4.0;

    // Mask thumbnail (click selects the topmost effect).
    if let Some(mask) = slot.mask {
        let mask_rect = Rect::from_min_size(pos2(x, rect.center().y - THUMB / 2.0), vec2(THUMB, THUMB));
        let top_paint = p.graph.get(mask).and_then(|m| m.mask_slots.iter().rev().find(|s| s.visible)).and_then(|s| match s.source {
            MaskSource::Node(n) if p.graph.get(n).is_some_and(|n| crate::graph::is_paint_mask(&n.ty)) => Some(n),
            _ => None,
        });
        match top_paint.and_then(|n| thumbs.get(ui.ctx(), gpu, p, n)) {
            Some(tex) => {
                painter.image(tex, mask_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => mask_thumb(&painter, p, mask, mask_rect),
        }
        if mask_selected {
            painter.rect_stroke(mask_rect.expand(1.0), 4.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
        }
        let mr = ui.interact(mask_rect, ui.id().with(("mask thumb", l.stack, l.index)), Sense::click());
        if mr.clicked() {
            let n = p.graph.get(mask).map(|m| m.mask_slots.len()).unwrap_or(0);
            if n > 0 {
                p.multi.clear();
                p.selection = Some(Selection::MaskLayer(l, n - 1));
            }
        }
        mr.on_hover_text("Mask: click to select its top effect");
        x += THUMB + 6.0;
    } else {
        x += 2.0;
    }
    // Type icon: the kind is an icon, not a subtitle.
    let icon = match kind {
        LayerKind::Paint | LayerKind::Paint2d => Icon::Brush,
        LayerKind::Fill => Icon::Fill,
        LayerKind::Group => Icon::Folder,
        LayerKind::Graph => Icon::Nodes,
        LayerKind::SmartMaterial => Icon::Material,
    };
    let icon_rect = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(13.0, 13.0));
    icons::paint(&painter, icon, icon_rect, TEXT_DIM);
    if kind == LayerKind::Paint2d {
        two_d_badge(&painter, icon_rect);
    }
    let kind_tip = ui.interact(icon_rect, ui.id().with(("kind", l.stack, l.index)), Sense::hover());
    kind_tip.on_hover_text(match kind {
        LayerKind::Group => format!("Folder · {} layers", p.graph.get(slot.content.unwrap_or(0)).map(|n| n.slots.len()).unwrap_or(0)),
        LayerKind::SmartMaterial => "Smart material".into(),
        k => k.name().into(),
    });
    x += 18.0;

    // Name, kind, and the blend mode / opacity readout for the viewed channel.
    let ch = view_channel(ui);
    let cb = slot.channels[ch.index()];
    let (mode, normal, opacity) = if cb.enabled {
        (short_mode(cb.mode), cb.mode == BlendMode::Normal, format!("{:.0}", cb.opacity * slot.opacity * 100.0))
    } else {
        ("off", true, String::new())
    };
    let name_color = if content_selected { Color32::WHITE } else { TEXT };
    let name_width = (rect.right() - OPACITY_W - BLEND_W - x - 6.0).max(10.0);
    let mut job = egui::text::LayoutJob::simple_singleline(slot.name.clone(), t::body_strong(), name_color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(name_width);
    let name_galley = painter.layout_job(job);
    painter.galley(pos2(x, rect.center().y - name_galley.size().y / 2.0), name_galley, name_color);
    paint_columns(&painter, rect, mode, normal, &opacity, !cb.enabled);
    painter.set_opacity(1.0);

    // Selection and context menu.
    if resp.clicked() {
        if ui.input(|i| i.modifiers.command) {
            if let Some(prev) = p.selection.map(|s| s.layer()).filter(|pl| pl.stack == l.stack && *pl != l) {
                if !p.multi.contains(&prev) {
                    p.multi.push(prev);
                }
            }
            p.multi.retain(|m| m.stack == l.stack);
            if let Some(i) = p.multi.iter().position(|m| *m == l) {
                p.multi.remove(i);
            } else {
                p.selection = Some(Selection::Layer(l));
            }
        } else {
            p.multi.clear();
            p.selection = Some(Selection::Layer(l));
        }
    }
    if resp.double_clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("rename"), (l.stack, l.index, slot.name.clone())));
    }
    resp.context_menu(|ui| {
        p.selection = Some(Selection::Layer(l));
        ui_layers::layer_context_menu(ui, gpu, p, l, actions);
    });

    // Drag to reorder: drop on the upper half to place above, lower half below.
    if resp.drag_started() {
        resp.dnd_set_drag_payload(l);
    }
    if let Some(dragged) = resp.dnd_hover_payload::<LayerRef>() {
        if *dragged != l {
            let above = ui.input(|i| i.pointer.hover_pos()).is_some_and(|pt| pt.y < rect.center().y);
            let y = if above { rect.top() } else { rect.bottom() };
            ui.painter().line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(2.0, DROP));
        }
    }
    if let Some(dragged) = crate::ui_widgets::dnd_release::<LayerRef>(&resp) {
        let dragged = *dragged;
        if dragged != l {
            let above = ui.input(|i| i.pointer.interact_pos()).is_some_and(|pt| pt.y < rect.center().y);
            let moved = p.edit(gpu, "Move layer", |g| if above { ops::move_above(g, dragged, l) } else { ops::move_below(g, dragged, l) });
            p.selection = Some(Selection::Layer(moved));
        }
    }
    if let Some(path) = crate::ui_widgets::dnd_release::<std::path::PathBuf>(&resp) {
        actions.insert_smart_material = Some((*path).clone());
    }
    ui_layers::rename_popup(ui, gpu, p, l);
}

fn effect_rows(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef, mask: NodeId, depth: usize) {
    let slots = p.graph.get(mask).map(|m| m.mask_slots.clone()).unwrap_or_default();
    for i in (0..slots.len()).rev() {
        let m = &slots[i];
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(width, EFFECT_H), Sense::click());
        let selected = p.selection == Some(Selection::MaskLayer(l, i));
        let mut painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, if resp.hovered() { ROW_HOVER } else { EFFECT });
        if selected {
            paint_selected(&painter, rect);
        }
        painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
        painter.vline(rect.left() + EYE_W - 0.5, rect.y_range(), Stroke::new(1.0, t::DIVIDER));

        let eye_rect = Rect::from_center_size(pos2(rect.left() + EYE_W / 2.0, rect.center().y), vec2(16.0, 16.0));
        let eye = ui.interact(eye_rect, ui.id().with(("fx eye", l.stack, l.index, i)), Sense::click());
        let (eye_icon, eye_color) = if m.visible { (Icon::Visible, t::TEXT_2) } else { (Icon::Hidden, Color32::from_rgb(0x4a, 0x4d, 0x55)) };
        icons::paint(&painter, eye_icon, eye_rect.shrink(2.0), eye_color);
        if !m.visible {
            painter.set_opacity(HIDDEN_OPACITY);
        }
        if eye.clicked() {
            p.edit(gpu, "Toggle effect", |g| {
                if let Some(ms) = ops::mask_slot_mut(g, l, i) {
                    ms.visible = !ms.visible;
                }
            });
        }
        let x = rect.left() + EYE_W + 6.0 + depth as f32 * INDENT + 24.0;
        // Tree connector from the layer's mask thumbnail.
        painter.line_segment([pos2(x - 12.0, rect.top()), pos2(x - 12.0, rect.center().y)], Stroke::new(1.0, t::BORDER_STRONG));
        painter.line_segment([pos2(x - 12.0, rect.center().y), pos2(x - 4.0, rect.center().y)], Stroke::new(1.0, t::BORDER_STRONG));
        let icon = match &m.source {
            MaskSource::Levels { .. } => Icon::Levels,
            MaskSource::Node(n) => match p.graph.get(*n).map(|n| n.ty.as_str()) {
                Some(ty) if crate::graph::is_paint_mask(ty) => Icon::Brush,
                Some("value") | Some("image") => Icon::Fill,
                _ => Icon::Generator,
            },
        };
        let icon_rect = Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0));
        icons::paint(&painter, icon, icon_rect, t::TEXT_2);
        // Fill effects show their value as a swatch.
        if let MaskSource::Node(n) = &m.source {
            if let Some(node) = p.graph.get(*n).filter(|n| n.ty == "value") {
                let g = (node.value().clamp(0.0, 1.0) * 255.0) as u8;
                let sw = Rect::from_center_size(icon_rect.center(), vec2(12.0, 12.0));
                painter.rect_filled(sw, 2.0, Color32::from_gray(g));
                painter.rect_stroke(sw, 2.0, Stroke::new(1.0, Color32::from_gray(110)), StrokeKind::Outside);
            }
        }
        painter.text(pos2(x + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, &m.name, t::body(), TEXT);
        paint_columns(&painter, rect, short_mode(m.mode), m.mode == BlendMode::Normal, &format!("{:.0}", m.opacity * 100.0), false);
        painter.set_opacity(1.0);
        if resp.clicked() {
            p.multi.clear();
            p.selection = Some(Selection::MaskLayer(l, i));
        }
        resp.context_menu(|ui| {
            ui_layers::mask_add_menu(ui, gpu, p, l);
            ui.separator();
            if ui.button("Delete effect").clicked() {
                p.edit(gpu, "Delete mask layer", |g| ops::delete_mask_layer(g, l, i));
                p.selection = Some(Selection::Layer(l));
                ui.close();
            }
        });
    }
}
