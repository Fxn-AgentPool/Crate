//! The Layers panel list: tall rows with content and mask thumbnails, blend mode and opacity
//! readouts for a chosen channel, nested effects (mask layers), folders and drag-to-reorder.
//! See Painter-docs/reference/layers.md.

use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::graph::{BlendMode, MaskSource, NodeId};
use crate::ops::{self, LayerKind, LayerRef, Selection};
use crate::project::Project;
use crate::thumbs::Thumbnails;
use crate::ui_layers::{self, PanelActions};

const ROW_H: f32 = 44.0;
const EFFECT_H: f32 = 24.0;
const THUMB: f32 = 34.0;
const INDENT: f32 = 14.0;

const BG: Color32 = Color32::from_rgb(34, 34, 36);
const ROW: Color32 = Color32::from_rgb(46, 46, 49);
const ROW_HOVER: Color32 = Color32::from_rgb(54, 54, 58);
const ROW_SELECTED: Color32 = Color32::from_rgb(56, 74, 100);
const EFFECT: Color32 = Color32::from_rgb(40, 40, 43);
const TEXT: Color32 = Color32::from_rgb(220, 220, 222);
const TEXT_DIM: Color32 = Color32::from_rgb(140, 140, 146);
const ACCENT: Color32 = Color32::from_rgb(240, 240, 240);
const DROP: Color32 = Color32::from_rgb(90, 160, 240);

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

/// Strip header in the style of the panels ("LAYERS", "PROPERTIES").
pub fn panel_header(ui: &mut egui::Ui, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(30, 30, 32));
    ui.painter().text(
        rect.left_center() + vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title.to_uppercase(),
        FontId::proportional(11.0),
        Color32::from_gray(170),
    );
}

pub fn layers_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, actions: &mut PanelActions, thumbs: &mut Thumbnails) {
    toolbar(ui, gpu, p);
    ui.add_space(2.0);
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0.0, BG);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
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
            if let Some(path) = r.dnd_release_payload::<std::path::PathBuf>() {
                actions.insert_smart_material = Some((*path).clone());
            }
        }
    });
}

// ------------------------------------------------------------------ toolbar

#[derive(Clone, Copy)]
enum Icon {
    Brush,
    Bucket,
    Folder,
    Mask,
    Effect,
    Duplicate,
    Trash,
    Nodes,
    Eye,
    Levels,
    Star,
    Gradient,
}

fn draw_icon(p: &egui::Painter, icon: Icon, r: Rect, color: Color32) {
    let c = r.center();
    let s = r.width().min(r.height()) * 0.5;
    let st = Stroke::new(1.4, color);
    match icon {
        Icon::Brush => {
            // Handle and tip.
            p.line_segment([c + vec2(s * 0.7, -s * 0.7), c + vec2(-s * 0.1, s * 0.1)], Stroke::new(2.0, color));
            p.circle_filled(c + vec2(-s * 0.35, s * 0.35), s * 0.32, color);
        }
        Icon::Bucket => {
            let pts = vec![c + vec2(-s * 0.6, -s * 0.2), c + vec2(0.0, -s * 0.7), c + vec2(s * 0.6, -s * 0.1), c + vec2(0.0, s * 0.6)];
            p.add(egui::Shape::convex_polygon(pts, Color32::TRANSPARENT, st));
            p.circle_filled(c + vec2(s * 0.75, s * 0.45), s * 0.18, color);
        }
        Icon::Folder => {
            let body = Rect::from_min_max(c + vec2(-s * 0.75, -s * 0.35), c + vec2(s * 0.75, s * 0.6));
            p.rect_stroke(body, 2.0, st, StrokeKind::Inside);
            p.line_segment([c + vec2(-s * 0.75, -s * 0.35), c + vec2(-s * 0.35, -s * 0.65)], st);
            p.line_segment([c + vec2(-s * 0.35, -s * 0.65), c + vec2(0.0, -s * 0.65)], st);
        }
        Icon::Mask => {
            let rr = Rect::from_center_size(c, vec2(s * 1.5, s * 1.5));
            p.rect_stroke(rr, 2.0, st, StrokeKind::Inside);
            p.circle_filled(c, s * 0.42, color);
        }
        Icon::Effect => {
            // Four-point sparkle.
            for (a, b) in [(vec2(0.0, -s * 0.8), vec2(0.0, s * 0.8)), (vec2(-s * 0.8, 0.0), vec2(s * 0.8, 0.0))] {
                p.line_segment([c + a, c + b], st);
            }
            p.circle_filled(c, s * 0.22, color);
        }
        Icon::Duplicate => {
            p.rect_stroke(Rect::from_min_size(c + vec2(-s * 0.7, -s * 0.7), vec2(s * 1.0, s * 1.0)), 1.5, st, StrokeKind::Inside);
            p.rect_stroke(Rect::from_min_size(c + vec2(-s * 0.3, -s * 0.3), vec2(s * 1.0, s * 1.0)), 1.5, st, StrokeKind::Inside);
        }
        Icon::Trash => {
            p.rect_stroke(Rect::from_min_max(c + vec2(-s * 0.5, -s * 0.35), c + vec2(s * 0.5, s * 0.75)), 1.5, st, StrokeKind::Inside);
            p.line_segment([c + vec2(-s * 0.75, -s * 0.5), c + vec2(s * 0.75, -s * 0.5)], st);
            p.line_segment([c + vec2(-s * 0.2, -s * 0.75), c + vec2(s * 0.2, -s * 0.75)], st);
        }
        Icon::Nodes => {
            let a = c + vec2(-s * 0.55, -s * 0.4);
            let b = c + vec2(s * 0.55, 0.0);
            let d = c + vec2(-s * 0.55, s * 0.45);
            p.line_segment([a, b], st);
            p.line_segment([d, b], st);
            for q in [a, b, d] {
                p.circle_filled(q, s * 0.22, color);
            }
        }
        Icon::Eye => {
            let w = s * 0.9;
            let h = s * 0.55;
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0 * std::f32::consts::TAU;
                    c + vec2(t.cos() * w, t.sin() * h * (1.0 - 0.35 * t.cos().abs()))
                })
                .collect();
            p.add(egui::Shape::closed_line(pts, st));
            p.circle_filled(c, s * 0.28, color);
        }
        Icon::Levels => {
            for (i, h) in [0.4f32, 0.9, 0.6, 0.3].iter().enumerate() {
                let x = c.x - s * 0.6 + i as f32 * s * 0.4;
                p.line_segment([pos2(x, c.y + s * 0.6), pos2(x, c.y + s * 0.6 - h * s * 1.2)], Stroke::new(2.0, color));
            }
        }
        Icon::Gradient => {
            let rr = Rect::from_center_size(c, vec2(s * 1.6, s * 1.2));
            let n = 8;
            for i in 0..n {
                let t = i as f32 / (n - 1) as f32;
                let x0 = rr.left() + rr.width() * i as f32 / n as f32;
                let strip = Rect::from_min_max(pos2(x0, rr.top()), pos2(x0 + rr.width() / n as f32 + 0.5, rr.bottom()));
                let g = (40.0 + t * 200.0) as u8;
                p.rect_filled(strip, 0.0, Color32::from_gray(g));
            }
            p.rect_stroke(rr, 1.0, Stroke::new(1.0, color), StrokeKind::Outside);
        }
        Icon::Star => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = i as f32 / 10.0 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                    let rad = if i % 2 == 0 { s * 0.8 } else { s * 0.35 };
                    c + vec2(a.cos() * rad, a.sin() * rad)
                })
                .collect();
            p.add(egui::Shape::closed_line(pts, st));
        }
    }
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), if enabled { Sense::click() } else { Sense::hover() });
    let color = if !enabled {
        Color32::from_gray(80)
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        Color32::from_gray(190)
    };
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 3.0, Color32::from_gray(60));
    }
    draw_icon(ui.painter(), icon, rect.shrink(4.0), color);
    resp.on_hover_text(tip)
}

fn toolbar(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
    let sel = p.selection.map(|s| s.layer());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        if icon_button(ui, Icon::Brush, "Add paint layer (Ctrl+Shift+N)", true).clicked() {
            ui_layers::add_paint(gpu, p);
        }
        if icon_button(ui, Icon::Bucket, "Add fill layer (Ctrl+Shift+F)", true).clicked() {
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
        let fx_resp = icon_button(ui, Icon::Gradient, "Add effect to the mask (generator, paint, fill, levels)", sel.is_some());
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
        // Channel shown in the rows' blend mode / opacity readout.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut ch = view_channel(ui);
            egui::ComboBox::from_id_salt("layer view channel combo")
                .width(96.0)
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
        });
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

/// Paints a content thumbnail for a layer.
fn content_thumb(painter: &egui::Painter, p: &Project, kind: LayerKind, content: Option<NodeId>, r: Rect) {
    painter.rect_filled(r, 2.0, Color32::from_gray(24));
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
                draw_icon(painter, Icon::Bucket, r.shrink(8.0), Color32::from_gray(150));
            }
        }
        LayerKind::Paint => {
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
            draw_icon(painter, Icon::Brush, r.shrink(8.0), Color32::from_gray(200));
        }
        LayerKind::Group => draw_icon(painter, Icon::Folder, r.shrink(7.0), Color32::from_gray(200)),
        LayerKind::Graph => draw_icon(painter, Icon::Nodes, r.shrink(8.0), Color32::from_gray(200)),
        LayerKind::SmartMaterial => draw_icon(painter, Icon::Star, r.shrink(8.0), Color32::from_rgb(230, 190, 90)),
    }
}

/// Paints a mask thumbnail: the base fill value with a mark for what sits on top.
fn mask_thumb(painter: &egui::Painter, p: &Project, mask: NodeId, r: Rect) {
    let Some(ms) = p.graph.get(mask) else { return };
    let base = ms.mask_slots.first().and_then(|s| match s.source {
        MaskSource::Node(n) => p.graph.get(n).filter(|n| n.ty == "constant").map(|n| n.param("value").as_f32()),
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
                Some("paint_mask") => Some(Icon::Brush),
                Some("constant") => None,
                Some(_) => Some(Icon::Gradient),
                None => None,
            },
        };
        if let Some(icon) = icon {
            draw_icon(painter, icon, r.shrink(9.0), mark);
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
    let painter = ui.painter_at(rect);
    let bg = if selected {
        ROW_SELECTED
    } else if resp.hovered() {
        ROW_HOVER
    } else {
        ROW
    };
    painter.rect_filled(rect, 0.0, bg);
    if depth > 0 {
        // Folder contents: a guide line on the left.
        let x = rect.left() + 30.0 + (depth as f32 - 0.5) * INDENT;
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(70)));
    }

    // Visibility.
    let eye_rect = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(20.0, 20.0));
    let eye = ui.interact(eye_rect, ui.id().with(("eye", l.stack, l.index)), Sense::click());
    draw_icon(&painter, Icon::Eye, eye_rect.shrink(2.0), if slot.visible { TEXT } else { Color32::from_gray(70) });
    if eye.clicked() {
        p.edit(gpu, "Toggle visibility", |g| {
            if let Some(s) = ops::slot_mut(g, l) {
                s.visible = !s.visible;
            }
        });
    }
    painter.line_segment([pos2(rect.left() + 28.0, rect.top() + 6.0), pos2(rect.left() + 28.0, rect.bottom() - 6.0)], Stroke::new(1.0, Color32::from_gray(64)));

    let mut x = rect.left() + 32.0 + depth as f32 * INDENT;
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
        painter.add(egui::Shape::convex_polygon(pts, if arrow.hovered() { Color32::WHITE } else { TEXT_DIM }, Stroke::NONE));
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
        (LayerKind::Paint, Some(c)) => thumbs.get(ui.ctx(), gpu, p, c),
        _ => None,
    };
    match paint_thumb {
        Some(tex) => {
            painter.image(tex, content_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => content_thumb(&painter, p, kind, slot.content, content_rect),
    }
    if content_selected {
        painter.rect_stroke(content_rect.expand(1.0), 3.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
    }
    x += THUMB + 4.0;

    // Mask thumbnail (click selects the topmost effect).
    if let Some(mask) = slot.mask {
        let mask_rect = Rect::from_min_size(pos2(x, rect.center().y - THUMB / 2.0), vec2(THUMB, THUMB));
        let top_paint = p.graph.get(mask).and_then(|m| m.mask_slots.iter().rev().find(|s| s.visible)).and_then(|s| match s.source {
            MaskSource::Node(n) if p.graph.get(n).is_some_and(|n| n.ty == "paint_mask") => Some(n),
            _ => None,
        });
        match top_paint.and_then(|n| thumbs.get(ui.ctx(), gpu, p, n)) {
            Some(tex) => {
                painter.image(tex, mask_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => mask_thumb(&painter, p, mask, mask_rect),
        }
        if mask_selected {
            painter.rect_stroke(mask_rect.expand(1.0), 3.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
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

    // Name, kind, and the blend mode / opacity readout for the viewed channel.
    let ch = view_channel(ui);
    let cb = slot.channels[ch.index()];
    let readout = if cb.enabled {
        format!("{} {:.0}", short_mode(cb.mode), cb.opacity * slot.opacity * 100.0)
    } else {
        "off".to_string()
    };
    let right = rect.right() - 8.0;
    let readout_galley = painter.layout_no_wrap(readout, FontId::proportional(11.0), if cb.enabled { TEXT_DIM } else { Color32::from_gray(90) });
    let readout_pos = pos2(right - readout_galley.size().x, rect.top() + 7.0);
    let name_color = if slot.visible { TEXT } else { TEXT_DIM };
    let name_width = (readout_pos.x - x - 6.0).max(10.0);
    let name_galley = painter.layout(slot.name.clone(), FontId::proportional(13.0), name_color, name_width);
    painter.galley(pos2(x, rect.top() + 6.0), name_galley, name_color);
    painter.galley(readout_pos, readout_galley, TEXT_DIM);
    let sub = match kind {
        LayerKind::Group => format!("Folder · {} layers", p.graph.get(slot.content.unwrap_or(0)).map(|n| n.slots.len()).unwrap_or(0)),
        LayerKind::SmartMaterial => "Smart material".into(),
        k => k.name().into(),
    };
    painter.text(pos2(x, rect.bottom() - 7.0), egui::Align2::LEFT_BOTTOM, sub, FontId::proportional(10.5), TEXT_DIM);

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
    if let Some(dragged) = resp.dnd_release_payload::<LayerRef>() {
        let dragged = *dragged;
        if dragged != l {
            let above = ui.input(|i| i.pointer.interact_pos()).is_some_and(|pt| pt.y < rect.center().y);
            let moved = p.edit(gpu, "Move layer", |g| if above { ops::move_above(g, dragged, l) } else { ops::move_below(g, dragged, l) });
            p.selection = Some(Selection::Layer(moved));
        }
    }
    if let Some(path) = resp.dnd_release_payload::<std::path::PathBuf>() {
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
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, if selected { ROW_SELECTED } else if resp.hovered() { ROW_HOVER } else { EFFECT });

        let eye_rect = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(16.0, 16.0));
        let eye = ui.interact(eye_rect, ui.id().with(("fx eye", l.stack, l.index, i)), Sense::click());
        draw_icon(&painter, Icon::Eye, eye_rect.shrink(2.0), if m.visible { TEXT } else { Color32::from_gray(70) });
        if eye.clicked() {
            p.edit(gpu, "Toggle effect", |g| {
                if let Some(ms) = ops::mask_slot_mut(g, l, i) {
                    ms.visible = !ms.visible;
                }
            });
        }
        let x = rect.left() + 32.0 + depth as f32 * INDENT + 24.0;
        // Tree connector from the layer's mask thumbnail.
        painter.line_segment([pos2(x - 12.0, rect.top()), pos2(x - 12.0, rect.center().y)], Stroke::new(1.0, Color32::from_gray(80)));
        painter.line_segment([pos2(x - 12.0, rect.center().y), pos2(x - 4.0, rect.center().y)], Stroke::new(1.0, Color32::from_gray(80)));
        let icon = match &m.source {
            MaskSource::Levels { .. } => Icon::Levels,
            MaskSource::Node(n) => match p.graph.get(*n).map(|n| n.ty.as_str()) {
                Some("paint_mask") => Icon::Brush,
                Some("constant") | Some("image") => Icon::Bucket,
                _ => Icon::Gradient,
            },
        };
        let icon_rect = Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0));
        draw_icon(&painter, icon, icon_rect, Color32::from_gray(190));
        // Fill effects show their value as a swatch.
        if let MaskSource::Node(n) = &m.source {
            if let Some(node) = p.graph.get(*n).filter(|n| n.ty == "constant") {
                let g = (node.param("value").as_f32().clamp(0.0, 1.0) * 255.0) as u8;
                let sw = Rect::from_center_size(icon_rect.center(), vec2(12.0, 12.0));
                painter.rect_filled(sw, 2.0, Color32::from_gray(g));
                painter.rect_stroke(sw, 2.0, Stroke::new(1.0, Color32::from_gray(110)), StrokeKind::Outside);
            }
        }
        painter.text(pos2(x + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, &m.name, FontId::proportional(12.0), if m.visible { TEXT } else { TEXT_DIM });
        painter.text(
            pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{} {:.0}", short_mode(m.mode), m.opacity * 100.0),
            FontId::proportional(11.0),
            TEXT_DIM,
        );
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
