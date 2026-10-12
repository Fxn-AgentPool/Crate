//! The Curve and Geo Fill tools in the viewport, and the Curves section of the Properties panel.
//! See docs/reference/curves.md and docs/reference/brushes-tools-stencils.md#geo-fill.

use egui::{Color32, Pos2, RichText, Stroke, pos2, vec2};
use glam::{Mat4, Vec2, Vec3};

use crate::camera::Camera;
use crate::channels::Channel;
use crate::curve::{self, Curve, CurveSelection};
use crate::geo::Element;
use crate::gpu::Gpu;
use crate::graph::NodeId;
use crate::icons::Icon;
use crate::paint::PaintEngine;
use crate::project::Project;
use crate::ui_layers::track;
use crate::ui_theme as t;
use crate::ui_widgets::{self as w, Check, Field, Section, Toggle};

/// How close the pointer must be to a curve or a point to hit it, in points.
const HIT: f32 = 6.0;
/// Geo Fill highlights elements of up to this many triangles.
const HIGHLIGHT_MAX: usize = 50_000;

/// A 3D pane: its camera, where it is on screen, and its size in pixels.
#[derive(Clone, Copy)]
pub struct Pane3d {
    pub camera: Camera,
    pub rect: egui::Rect,
    pub ppp: f32,
    pub size: (u32, u32),
}

impl Pane3d {
    fn to_px(self, p: Pos2) -> Vec2 {
        Vec2::new((p.x - self.rect.min.x) * self.ppp, (p.y - self.rect.min.y) * self.ppp)
    }

    fn view_proj(&self, p: &Project) -> Mat4 {
        crate::viewport::Viewport::view_proj(&self.camera, &p.mesh, self.size.0, self.size.1)
    }

    /// Screen position (points) of an object-space point, if it is in front of the camera.
    fn project(&self, vp: &Mat4, p: Vec3) -> Option<Pos2> {
        let c = *vp * p.extend(1.0);
        if c.w <= 1e-6 {
            return None;
        }
        let n = c.truncate() / c.w;
        let (w, h) = (self.size.0 as f32 / self.ppp, self.size.1 as f32 / self.ppp);
        Some(pos2(self.rect.min.x + (n.x * 0.5 + 0.5) * w, self.rect.min.y + (0.5 - n.y * 0.5) * h))
    }
}

/// Viewport state of the Curve and Geo Fill tools.
#[derive(Default)]
pub struct CurveTool {
    /// The point of the selected curve being dragged, and whether it has moved (the undo step is
    /// made on the first move, so a click alone makes none).
    drag: Option<(usize, bool)>,
    /// The Geo Fill element under the pointer, the mesh it is of, and its triangles.
    hover: Option<(Element, u64, Vec<u32>)>,
}

/// Distance from `q` to a polyline of projected points (None: behind the camera).
fn polyline_distance(q: Pos2, pts: &[Option<Pos2>]) -> f32 {
    let mut best = f32::MAX;
    for w in pts.windows(2) {
        if let [Some(a), Some(b)] = w {
            let ab = *b - *a;
            let t = ((q - *a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            best = best.min((*a + ab * t - q).length());
        }
    }
    if let [Some(a)] = pts {
        best = (*a - q).length();
    }
    best
}

impl CurveTool {
    /// Whether a curve point is being dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The target's curves as screen polylines, by curve index.
    fn screen_paths(p: &Project, pane: &Pane3d, node: NodeId) -> Vec<Vec<Option<Pos2>>> {
        let vp = pane.view_proj(p);
        let Some(slot) = p.curve_slots.get(&node) else { return Vec::new() };
        slot.paths.iter().map(|path| path.iter().map(|s| pane.project(&vp, s.pos)).collect()).collect()
    }

    /// The selected curve's points on screen.
    fn screen_points(p: &Project, pane: &Pane3d, c: &Curve) -> Vec<Option<Pos2>> {
        let vp = pane.view_proj(p);
        c.points.iter().map(|q| pane.project(&vp, Vec3::from(q.pos))).collect()
    }

    /// Clicks and drags of the Curve tool in a 3D pane (docs/reference/curves.md#drawing and
    /// #editing). Returns a message for the status bar.
    #[allow(clippy::too_many_arguments)]
    pub fn input(&mut self, gpu: &Gpu, p: &mut Project, paint: &PaintEngine, pane: &Pane3d, hover: Option<Pos2>, pressed: bool, down: bool, ctrl: bool) -> Option<String> {
        if let Some((k, moved)) = self.drag {
            if !down {
                self.drag = None;
            } else if let Some(point) = hover.and_then(|h| curve::surface_point(p, &pane.camera, pane.size, pane.to_px(h))) {
                let same = curve::selected(p).is_some_and(|(n, i)| p.graph.node(n).curves[i].points.get(k) == Some(&point));
                if same {
                    return None;
                }
                if !moved {
                    p.checkpoint("Move curve point");
                    self.drag = Some((k, true));
                }
                if let Err(e) = curve::move_point(gpu, p, k, point) {
                    self.drag = None;
                    return Some(e);
                }
            }
            return None;
        }
        if !pressed {
            return None;
        }
        let h = hover?;
        let node = match curve::target(p) {
            Ok(n) => n,
            Err(e) => return Some(e),
        };
        let surface = |p: &Project| curve::surface_point(p, &pane.camera, pane.size, pane.to_px(h));
        let selected = curve::selected(p).filter(|(n, _)| *n == node).map(|(_, i)| i);
        let points = selected.map(|i| Self::screen_points(p, pane, &p.graph.node(node).curves[i])).unwrap_or_default();
        let hit_point = points.iter().enumerate().filter_map(|(k, q)| Some((k, (*q)? .distance(h)))).filter(|(_, d)| *d <= HIT).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(k, _)| k);
        let result = (|| -> Result<(), String> {
            if p.curve_sel.is_some_and(|s| s.drawing && s.node == node) {
                if hit_point == Some(0) && points.len() >= 3 {
                    return curve::close(gpu, p);
                }
                if let Some(point) = surface(p) {
                    curve::insert_point(gpu, p, points.len(), point)?;
                }
                return Ok(());
            }
            let paths = Self::screen_paths(p, pane, node);
            if ctrl {
                let Some(i) = selected else { return Ok(()) };
                if let Some(k) = hit_point {
                    return curve::delete_point(gpu, p, k);
                }
                if paths.get(i).is_some_and(|path| polyline_distance(h, path) <= HIT) {
                    let Some(point) = surface(p) else { return Ok(()) };
                    // Between the two points whose segment passes nearest.
                    let c = &p.graph.node(node).curves[i];
                    let n = c.points.len();
                    let segments = if c.loops() { n } else { n - 1 };
                    let at = Vec3::from(point.pos);
                    let nearest = (0..segments)
                        .map(|s| {
                            let (a, b) = (Vec3::from(c.points[s].pos), Vec3::from(c.points[(s + 1) % n].pos));
                            let t = ((at - a).dot(b - a) / (b - a).length_squared().max(1e-12)).clamp(0.0, 1.0);
                            (s, (a + (b - a) * t).distance(at))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map_or(n, |(s, _)| s + 1);
                    return curve::insert_point(gpu, p, nearest, point);
                }
                return Ok(());
            }
            if let Some(k) = hit_point {
                self.drag = Some((k, false));
                return Ok(());
            }
            // The selected curve first, then from the top.
            let order = selected.into_iter().chain((0..paths.len()).rev().filter(|i| Some(*i) != selected));
            if let Some(i) = order.into_iter().find(|&i| paths.get(i).is_some_and(|path| polyline_distance(h, path) <= HIT)) {
                return curve::select(p, Some(i));
            }
            let Some(first) = surface(p) else { return Ok(()) };
            let world = curve::world_per_px(&pane.camera, pane.size, Vec3::from(first.pos));
            let size = curve::brush_world_size(&paint.brush, Some(world));
            curve::start(gpu, p, first, &paint.brush, &paint.values, size).map(|_| ())
        })();
        result.err()
    }

    /// `Enter` / `Esc` finish the curve being drawn, `Backspace` removes its last point.
    pub fn keys(&mut self, gpu: &Gpu, p: &mut Project, ctx: &egui::Context) {
        if !p.curve_sel.is_some_and(|s| s.drawing) || ctx.egui_wants_keyboard_input() {
            return;
        }
        let (finish, back) = ctx.input_mut(|i| {
            let finish = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) | i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            (finish, i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        });
        if back {
            let _ = curve::remove_last_point(gpu, p);
        }
        if finish {
            curve::finish(gpu, p);
        }
    }

    /// The target's curves over a 3D pane (docs/reference/curves.md#display).
    pub fn draw(&self, ui: &egui::Ui, p: &Project, pane: &Pane3d, hover: Option<Pos2>) {
        let Ok(node) = curve::target(p) else { return };
        let painter = ui.painter_at(pane.rect);
        let selected = curve::selected(p).filter(|(n, _)| *n == node).map(|(_, i)| i);
        let line = |pts: &[Option<Pos2>], stroke: Stroke| {
            for w in pts.windows(2) {
                if let [Some(a), Some(b)] = w {
                    painter.line_segment([*a, *b], stroke);
                }
            }
        };
        let paths = Self::screen_paths(p, pane, node);
        for (i, path) in paths.iter().enumerate() {
            if Some(i) != selected {
                line(path, Stroke::new(1.0, Color32::from_white_alpha(170)));
            }
        }
        let Some(i) = selected else { return };
        let c = &p.graph.node(node).curves[i];
        if let Some(path) = paths.get(i) {
            line(path, Stroke::new(2.0, t::ACCENT));
        }
        let points = Self::screen_points(p, pane, c);
        if p.curve_sel.is_some_and(|s| s.drawing) {
            if let (Some(Some(last)), Some(h)) = (points.last(), hover) {
                painter.line_segment([*last, h], Stroke::new(1.0, t::accent(0.7)));
            }
        }
        for (k, q) in points.iter().enumerate() {
            let Some(q) = q else { continue };
            let half = if k == 0 { 4.5 } else { 3.0 };
            let r = egui::Rect::from_center_size(*q, vec2(half, half) * 2.0);
            painter.rect_filled(r, 1.0, Color32::from_black_alpha(160));
            painter.rect_stroke(r, 1.0, Stroke::new(1.5, t::ACCENT), egui::StrokeKind::Inside);
        }
    }

    /// Geo Fill: names the element under the pointer and, in a 3D pane, highlights it.
    #[allow(clippy::too_many_arguments)]
    pub fn geo_hover(&mut self, ui: &egui::Ui, p: &Project, paint: &PaintEngine, view: &crate::camera::View, rect: egui::Rect, ppp: f32, size: (u32, u32), hover: Pos2) {
        let px = Vec2::new((hover.x - rect.min.x) * ppp, (hover.y - rect.min.y) * ppp);
        let Some(e) = crate::geo::element_at(p, view, size, px, paint.fill_by) else { return };
        let key = p.mesh_version();
        if !self.hover.as_ref().is_some_and(|(he, hk, _)| *he == e && *hk == key) {
            self.hover = Some((e, key, p.geo().triangles(&p.mesh, e)));
        }
        let tris = &self.hover.as_ref().unwrap().2;
        let painter = ui.painter_at(rect);
        if let crate::camera::View::Persp(camera) = view {
            if tris.len() <= HIGHLIGHT_MAX {
                let pane = Pane3d { camera: *camera, rect, ppp, size };
                let vp = pane.view_proj(p);
                let eye = camera.eye();
                let mut mesh = egui::Mesh::default();
                let fill = t::accent(0.28);
                for &tri in tris {
                    let v = |k: usize| &p.mesh.vertices[p.mesh.indices[tri as usize * 3 + k] as usize];
                    let pos = [0, 1, 2].map(|k| Vec3::from(v(k).pos));
                    let n: Vec3 = (0..3).map(|k| Vec3::from(v(k).normal)).sum();
                    let centroid = (pos[0] + pos[1] + pos[2]) / 3.0;
                    if n.dot(eye - centroid) <= 0.0 {
                        continue;
                    }
                    let Some(s) = pos.iter().map(|q| pane.project(&vp, *q)).collect::<Option<Vec<_>>>() else { continue };
                    let base = mesh.vertices.len() as u32;
                    for q in s {
                        mesh.colored_vertex(q, fill);
                    }
                    mesh.add_triangle(base, base + 1, base + 2);
                }
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        let label = p.geo().label(&p.mesh, e, tris.len());
        let at = hover + vec2(16.0, 14.0);
        painter.text(at + vec2(1.0, 1.0), egui::Align2::LEFT_TOP, &label, t::value(), Color32::from_black_alpha(160));
        painter.text(at, egui::Align2::LEFT_TOP, label, t::value(), Color32::WHITE);
    }
}

/// ⊘ at the pointer: the tool can't paint here.
pub fn not_allowed(ui: &egui::Ui, at: Pos2) {
    let p = ui.painter();
    let r = 8.0;
    for (width, color) in [(3.0, Color32::from_black_alpha(140)), (1.5, Color32::WHITE)] {
        let s = Stroke::new(width, color);
        p.circle_stroke(at, r, s);
        p.line_segment([at + vec2(-r, r) * std::f32::consts::FRAC_1_SQRT_2, at + vec2(r, -r) * std::f32::consts::FRAC_1_SQRT_2], s);
    }
}

/// What Copy brush copies, and the 3D view it measures a Screen-size brush in.
pub struct BrushSource<'a> {
    pub paint: &'a mut PaintEngine,
    pub view: Option<(Camera, (u32, u32))>,
}

/// The Curves section of a paint layer or effect (docs/reference/curves.md#curves-in-the-properties-panel).
pub fn curves_section(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, src: &mut BrushSource) {
    let selected = curve::selected(p).filter(|(n, _)| *n == node).map(|(_, i)| i);
    let count = p.graph.node(node).curves.len();
    let mut state = (p, src);
    Section::new("Curves").id_salt("curves").show_with_state(
        ui,
        &mut state,
        |ui, (p, _)| {
            let Some(i) = selected else { return };
            let err = |r: Result<(), String>| r.err().map(|e| log::info!("{e}"));
            if ui.add(w::ghost_icon(Icon::Trash)).on_hover_text("Delete").clicked() {
                err(curve::delete(gpu, p));
            }
            if ui.add_enabled(i > 0, w::ghost("↓")).on_hover_text("Move down").clicked() {
                err(curve::reorder(gpu, p, false));
            }
            if ui.add_enabled(i + 1 < count, w::ghost("↑")).on_hover_text("Move up").clicked() {
                err(curve::reorder(gpu, p, true));
            }
        },
        |ui, (p, src)| curves_body(ui, gpu, p, node, src),
    );
}

fn curves_body(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, src: &mut BrushSource) {
    let curves = p.graph.node(node).curves.clone();
    if curves.is_empty() {
        w::hint(ui, "Draw a curve with the Curve tool (K).");
        return;
    }
    let selected = curve::selected(p).filter(|(n, _)| *n == node).map(|(_, i)| i);
    let renaming_id = ui.id().with(("curve rename", node));
    let mut renaming: Option<(usize, String)> = ui.data(|d| d.get_temp(renaming_id));
    // The top row is drawn on top.
    for i in (0..curves.len()).rev() {
        if let Some((ri, text)) = renaming.as_mut().filter(|(ri, _)| *ri == i) {
            let r = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
            r.request_focus();
            if r.lost_focus() {
                let name = text.trim().to_string();
                let ri = *ri;
                if !name.is_empty() && name != curves[ri].name {
                    p.edit(gpu, "Rename curve", |g| g.node_mut(node).curves[ri].name = name);
                }
                renaming = None;
            }
            continue;
        }
        let r = ui.selectable_label(selected == Some(i), RichText::new(&curves[i].name).font(t::value()));
        if r.double_clicked() {
            renaming = Some((i, curves[i].name.clone()));
        } else if r.clicked() {
            p.curve_sel = Some(CurveSelection { node, curve: Some(i), drawing: false });
        }
    }
    ui.data_mut(|d| match renaming {
        Some(r) => {
            d.insert_temp(renaming_id, r);
        }
        None => d.remove::<(usize, String)>(renaming_id),
    });

    ui.add_space(4.0);
    if let Some(i) = selected.filter(|i| *i < curves.len()) {
        curve_settings(ui, gpu, p, node, i, src);
    }
    ui.add_space(4.0);
    if ui.add(w::secondary("Rasterize")).on_hover_text("Merge the curves into the layer's paint").clicked() {
        if let Err(e) = src.paint.merge_curves(gpu, p) {
            log::warn!("{e}");
        }
    }
}

/// The selected curve's properties and values.
fn curve_settings(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, i: usize, src: &mut BrushSource) {
    let c = p.graph.node(node).curves[i].clone();
    let mask = crate::graph::is_paint_mask(&p.graph.node(node).ty);
    macro_rules! field {
        ($ui:expr, $name:literal, $field:ident, $range:expr, $decimals:expr) => {{
            let mut v = c.$field;
            let r = $ui.add(Field::new(&mut v, $range).label($name).decimals($decimals));
            track(p, gpu, &r, "Curve settings", |g| g.node_mut(node).curves[i].$field = v);
        }};
    }
    {
        let mut v = c.size;
        let r = ui.add(Field::new(&mut v, 0.0001..=1000.0).label("Size").decimals(3).speed(0.001).no_fill());
        track(p, gpu, &r, "Curve settings", |g| g.node_mut(node).curves[i].size = v);
    }
    field!(ui, "Hardness", hardness, 0.0..=1.0, 2);
    field!(ui, "Flow", flow, 0.0..=1.0, 2);
    field!(ui, "Opacity", opacity, 0.0..=1.0, 2);
    {
        let mut v = c.spacing;
        let r = ui.add(Field::new(&mut v, 1.0..=200.0).label("Spacing").decimals(0).suffix(" %").speed(0.5).no_fill());
        track(p, gpu, &r, "Curve settings", |g| g.node_mut(node).curves[i].spacing = v);
    }
    w::setting(ui, "Alpha", |ui| {
        let label = if c.alpha.is_empty() { "Round".to_string() } else { c.alpha.clone() };
        if ui.add(w::secondary(label)).clicked() {
            if let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"]).pick_file() {
                match p.import_image(gpu, &path) {
                    Ok(name) => p.edit(gpu, "Curve alpha", |g| g.node_mut(node).curves[i].alpha = name),
                    Err(e) => log::error!("{e:#}"),
                }
            }
        }
        if !c.alpha.is_empty() && ui.add(w::ghost_icon(Icon::Close)).on_hover_text("Round tip").clicked() {
            p.edit(gpu, "Curve alpha", |g| g.node_mut(node).curves[i].alpha.clear());
        }
    });
    field!(ui, "Angle", angle, 0.0..=360.0, 0);
    w::setting(ui, "Closed", |ui| {
        let mut v = c.closed;
        let r = ui.add_enabled(c.points.len() >= 3, Toggle(&mut v)).on_disabled_hover_text("A closed curve needs 3 or more points");
        track(p, gpu, &r, "Close curve", |g| g.node_mut(node).curves[i].closed = v);
    });
    Section::new("Jitter").id_salt("curve jitter").default_open(false).show(ui, |ui| {
        field!(ui, "Size", jitter_size, 0.0..=1.0, 2);
        field!(ui, "Angle", jitter_angle, 0.0..=1.0, 2);
        field!(ui, "Position", jitter_position, 0.0..=1.0, 2);
        field!(ui, "Flow", jitter_flow, 0.0..=1.0, 2);
    });
    Section::new("Values").id_salt("curve values").show(ui, |ui| {
        if mask {
            let mut v = c.values.mask_value;
            let r = ui.add(Field::new(&mut v, 0.0..=1.0).label("Mask value"));
            track(p, gpu, &r, "Curve values", |g| g.node_mut(node).curves[i].values.mask_value = v);
            return;
        }
        let enabled: Vec<Channel> = Channel::ALL.into_iter().filter(|ch| p.channels.is_enabled(*ch)).collect();
        for ch in enabled {
            let k = ch.index();
            ui.horizontal(|ui| {
                let mut on = c.values.enabled[k];
                let r = ui.add(Check::new(&mut on, "")).on_hover_text(format!("Paint {}", ch.name()));
                track(p, gpu, &r, "Curve values", |g| g.node_mut(node).curves[i].values.enabled[k] = on);
                ui.add_enabled_ui(c.values.enabled[k], |ui| {
                    w::setting(ui, ch.name(), |ui| {
                        let mut v = c.values.values[k];
                        let r = match ch {
                            Channel::BaseColor | Channel::Emissive => {
                                ui.spacing_mut().interact_size = vec2(ui.available_width(), t::H_CONTROL);
                                w::color_edit(ui, &mut v)
                            }
                            Channel::Normal => {
                                let fw = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(40.0);
                                let mut r = ui.add(Field::new(&mut v[0], 0.0..=1.0).decimals(2).width(fw));
                                for x in &mut v[1..] {
                                    r = r.union(ui.add(Field::new(x, 0.0..=1.0).decimals(2).width(fw)));
                                }
                                r
                            }
                            Channel::Height => ui.add(Field::new(&mut v[0], -1.0..=1.0)),
                            _ => ui.add(Field::new(&mut v[0], 0.0..=1.0)),
                        };
                        track(p, gpu, &r, "Curve values", |g| g.node_mut(node).curves[i].values.values[k] = v);
                    });
                });
            });
        }
    });
    if ui.add(w::secondary("Copy brush")).on_hover_text("Set the curve's settings and values from the current brush").clicked() {
        let first = c.points.first().map(|q| Vec3::from(q.pos));
        let world = src.view.zip(first).map(|((camera, size), at)| curve::world_per_px(&camera, size, at));
        let size = curve::brush_world_size(&src.paint.brush, world);
        let (brush, values) = (src.paint.brush.clone(), src.paint.values.clone());
        p.edit(gpu, "Copy brush", |g| g.node_mut(node).curves[i].copy_brush(&brush, &values, size));
    }
}
