//! Node Editor. See Painter-docs/reference/nodes.md and Painter-docs/how-to/edit-a-layer-as-a-graph.md.
//!
//! Shows the subgraph behind one layer: its content and mask, plus a virtual "Layer" sink
//! node whose inputs are the layer's content and mask. Connecting into the sink, into a
//! Stack's layer sockets or into a Mask Stack's sockets edits those slots.

use std::collections::BTreeSet;

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2, epaint::CubicBezierShape};

use crate::gpu::Gpu;
use crate::graph::{self, Graph, MaskSlot, MaskSource, NodeId, SocketType, Value};
use crate::ops::{self, LayerRef};
use crate::project::Project;

const SINK: NodeId = u32::MAX;
const NODE_W: f32 = 170.0;
const HEADER: f32 = 22.0;
const ROW: f32 = 20.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocketRef {
    node: NodeId,
    name: String,
}

#[derive(Clone, Debug)]
enum Drag {
    Node(NodeId),
    /// Dragging a new link from an output.
    Link { from: SocketRef, ty: SocketType },
    Pan,
}

pub struct NodeEditor {
    pub open: bool,
    pub layer: Option<LayerRef>,
    pan: Vec2,
    zoom: f32,
    selected: Option<NodeId>,
    drag: Option<Drag>,
    /// Nodes added in this editor session that are not yet connected.
    extra: BTreeSet<NodeId>,
    add_menu: Option<(Pos2, String)>,
    auto_layout_needed: bool,
    frame_needed: bool,
}

impl Default for NodeEditor {
    fn default() -> Self {
        Self {
            open: false,
            layer: None,
            pan: Vec2::ZERO,
            zoom: 1.0,
            selected: None,
            drag: None,
            extra: BTreeSet::new(),
            add_menu: None,
            auto_layout_needed: false,
            frame_needed: false,
        }
    }
}

fn type_color(t: SocketType) -> Color32 {
    match t {
        SocketType::Float => Color32::from_gray(170),
        SocketType::Color => Color32::from_rgb(230, 200, 60),
        SocketType::Material => Color32::from_rgb(90, 200, 110),
    }
}

/// Input sockets of a node, including slot sockets of stacks.
fn inputs(g: &Graph, id: NodeId) -> Vec<(String, String, SocketType)> {
    if id == SINK {
        return vec![
            ("content".into(), "Content".into(), SocketType::Material),
            ("mask".into(), "Mask".into(), SocketType::Float),
        ];
    }
    let n = g.node(id);
    let mut v: Vec<_> = n.def().inputs.iter().map(|(name, t)| (name.to_string(), pretty(name), *t)).collect();
    for (i, s) in n.slots.iter().enumerate() {
        v.push((format!("slot{i}"), s.name.clone(), SocketType::Material));
        v.push((format!("slot{i}.mask"), format!("{} mask", s.name), SocketType::Float));
    }
    for (i, m) in n.mask_slots.iter().enumerate() {
        if matches!(m.source, MaskSource::Node(_)) {
            v.push((format!("m{i}"), m.name.clone(), SocketType::Float));
        }
    }
    v
}

fn outputs(g: &Graph, id: NodeId) -> Vec<(String, SocketType)> {
    if id == SINK {
        return vec![];
    }
    g.node(id).def().outputs.iter().map(|(n, t)| (n.to_string(), *t)).collect()
}

fn pretty(s: &str) -> String {
    let mut out = s.replace('_', " ");
    if let Some(f) = out.get(0..1) {
        out.replace_range(0..1, &f.to_uppercase());
    }
    out
}

/// The default output used when a node is referenced without an output name (slots).
fn main_output(g: &Graph, id: NodeId) -> String {
    g.node(id).def().outputs.first().map(|(n, _)| n.to_string()).unwrap_or_else(|| "out".into())
}

/// All links into `id`: (input socket, source node, source output).
fn links_into(g: &Graph, layer: LayerRef, id: NodeId) -> Vec<(String, NodeId, String)> {
    let mut v = Vec::new();
    if id == SINK {
        if let Some(s) = ops::slot(g, layer) {
            if let Some(c) = s.content.filter(|c| g.get(*c).is_some()) {
                v.push(("content".into(), c, main_output(g, c)));
            }
            if let Some(m) = s.mask.filter(|c| g.get(*c).is_some()) {
                v.push(("mask".into(), m, main_output(g, m)));
            }
        }
        return v;
    }
    let n = g.node(id);
    for (input, l) in &n.inputs {
        v.push((input.clone(), l.node, l.output.clone()));
    }
    for (i, s) in n.slots.iter().enumerate() {
        if let Some(c) = s.content.filter(|c| g.get(*c).is_some()) {
            v.push((format!("slot{i}"), c, main_output(g, c)));
        }
        if let Some(m) = s.mask.filter(|c| g.get(*c).is_some()) {
            v.push((format!("slot{i}.mask"), m, main_output(g, m)));
        }
    }
    for (i, m) in n.mask_slots.iter().enumerate() {
        if let MaskSource::Node(src) = m.source {
            if g.get(src).is_some() {
                v.push((format!("m{i}"), src, main_output(g, src)));
            }
        }
    }
    v
}

/// Wraps a Float-producing node in a Mask Stack so it can be a layer mask.
fn as_mask_stack(g: &mut Graph, src: NodeId) -> NodeId {
    if g.node(src).ty == "mask_stack" {
        return src;
    }
    let ms = g.add("mask_stack");
    let name = g.node(src).title().to_string();
    g.node_mut(ms).mask_slots.push(MaskSlot {
        name,
        visible: true,
        opacity: 1.0,
        mode: graph::BlendMode::Normal,
        source: MaskSource::Node(src),
    });
    let p = g.node(src).pos;
    g.node_mut(ms).pos = [p[0] + 200.0, p[1]];
    ms
}

/// Applies a connection. Returns false if it is not allowed.
fn connect(g: &mut Graph, layer: LayerRef, from: &SocketRef, to: &SocketRef) -> bool {
    let Some(from_ty) = g.get(from.node).and_then(|_| g.output_type(&graph::Link { node: from.node, output: from.name.clone() })) else {
        return false;
    };
    let to_ty = match inputs(g, to.node).into_iter().find(|(n, _, _)| *n == to.name) {
        Some((_, _, t)) => t,
        None => return false,
    };
    let compatible = match (from_ty, to_ty) {
        (a, b) if a == b => true,
        (SocketType::Float, SocketType::Color) | (SocketType::Color, SocketType::Float) => true,
        _ => false,
    };
    if !compatible {
        return false;
    }
    let would_cycle = |g: &Graph, target: NodeId| target == from.node || g.depends_on(from.node, target);
    if to.node == SINK {
        if to.name == "content" {
            if let Some(s) = ops::slot_mut(g, layer) {
                s.content = Some(from.node);
            }
        } else {
            let ms = as_mask_stack(g, from.node);
            if let Some(s) = ops::slot_mut(g, layer) {
                s.mask = Some(ms);
            }
        }
        return true;
    }
    if would_cycle(g, to.node) {
        return false;
    }
    if let Some(rest) = to.name.strip_prefix("slot") {
        let (idx, is_mask) = match rest.strip_suffix(".mask") {
            Some(i) => (i, true),
            None => (rest, false),
        };
        let Ok(i) = idx.parse::<usize>() else { return false };
        if is_mask {
            let ms = as_mask_stack(g, from.node);
            g.node_mut(to.node).slots[i].mask = Some(ms);
        } else {
            g.node_mut(to.node).slots[i].content = Some(from.node);
        }
        return true;
    }
    if let Some(idx) = to.name.strip_prefix('m') {
        if g.node(to.node).ty == "mask_stack" {
            let Ok(i) = idx.parse::<usize>() else { return false };
            g.node_mut(to.node).mask_slots[i].source = MaskSource::Node(from.node);
            return true;
        }
    }
    g.connect(from.node, &from.name, to.node, &to.name)
}

fn disconnect(g: &mut Graph, layer: LayerRef, to: &SocketRef) {
    if to.node == SINK {
        if let Some(s) = ops::slot_mut(g, layer) {
            if to.name == "content" {
                s.content = None;
            } else {
                s.mask = None;
            }
        }
        return;
    }
    if let Some(rest) = to.name.strip_prefix("slot") {
        let (idx, is_mask) = match rest.strip_suffix(".mask") {
            Some(i) => (i, true),
            None => (rest, false),
        };
        if let Ok(i) = idx.parse::<usize>() {
            let s = &mut g.node_mut(to.node).slots[i];
            if is_mask {
                s.mask = None;
            } else {
                s.content = None;
            }
        }
        return;
    }
    g.disconnect(to.node, &to.name);
}

/// Replaces a generator node with equivalent primitive nodes (approximately; parameters
/// are copied as fixed values).
pub fn ungroup(g: &mut Graph, id: NodeId) -> Option<NodeId> {
    let n = g.node(id).clone();
    let f = |name: &str| n.param(name).as_f32();
    let pos = n.pos;
    let mut place = |g: &mut Graph, ty: &str, dx: f32, dy: f32| {
        let k = g.add(ty);
        g.node_mut(k).pos = [pos[0] + dx, pos[1] + dy];
        k
    };
    let result = match n.ty.as_str() {
        "edge_wear" => {
            let curv = place(g, "mesh_map", -520.0, 0.0);
            g.set(curv, "map", Value::Text("Curvature".into()));
            let convex = place(g, "levels", -340.0, 0.0);
            g.set(convex, "in_black", Value::Float(0.52));
            g.set(convex, "in_white", Value::Float(0.52 + 1.0 / 3.2));
            g.connect(curv, "float", convex, "in");
            let noise = place(g, "noise", -520.0, 140.0);
            g.set(noise, "scale", Value::Float(f("noise_scale")));
            g.set(noise, "seed", Value::Float(f("seed")));
            let nlev = place(g, "levels", -340.0, 140.0);
            g.set(nlev, "in_black", Value::Float(0.3));
            g.set(nlev, "in_white", Value::Float(0.7));
            g.connect(noise, "out", nlev, "in");
            let na = f("noise_amount");
            let scale = place(g, "math", -170.0, 140.0);
            g.set(scale, "op", Value::Text("Multiply".into()));
            g.set(scale, "b_value", Value::Float(2.0 * na));
            g.connect(nlev, "float", scale, "a");
            let offset = place(g, "math", 0.0, 140.0);
            g.set(offset, "op", Value::Text("Add".into()));
            g.set(offset, "b_value", Value::Float(1.0 - na));
            g.connect(scale, "out", offset, "a");
            let mul = place(g, "math", 170.0, 0.0);
            g.set(mul, "op", Value::Text("Multiply".into()));
            g.connect(convex, "float", mul, "a");
            g.connect(offset, "out", mul, "b");
            let t = (1.0 - 1.4 * f("amount")).max(0.05);
            let w = 0.5 * (1.0 - f("sharpness")) + 0.01;
            let out = place(g, "levels", 340.0, 0.0);
            g.set(out, "in_black", Value::Float((t - w).max(0.0)));
            g.set(out, "in_white", Value::Float(t + w));
            g.connect(mul, "out", out, "in");
            Some((out, "float"))
        }
        "dirt" => {
            let ao = place(g, "mesh_map", -340.0, 0.0);
            let inv = place(g, "invert", -170.0, 0.0);
            g.connect(ao, "color", inv, "in");
            let noise = place(g, "noise", -340.0, 140.0);
            g.set(noise, "scale", Value::Float(f("noise_scale")));
            g.set(noise, "seed", Value::Float(f("seed")));
            let add = place(g, "math", 0.0, 0.0);
            g.set(add, "op", Value::Text("Add".into()));
            g.connect(inv, "float", add, "a");
            g.connect(noise, "out", add, "b");
            let t = 1.0 - f("amount") + 0.5;
            let w = 0.5 * (1.0 - f("contrast")) + 0.02;
            let out = place(g, "levels", 170.0, 0.0);
            g.set(out, "in_black", Value::Float((t - w).max(0.0)));
            g.set(out, "in_white", Value::Float(t + w));
            g.connect(add, "out", out, "in");
            Some((out, "float"))
        }
        "curvature_mask" => {
            let curv = place(g, "mesh_map", -340.0, 0.0);
            g.set(curv, "map", Value::Text("Curvature".into()));
            let concave = n.param("select").as_str() == "Concave";
            let mut src = (curv, "float");
            if concave {
                let inv = place(g, "invert", -170.0, 0.0);
                g.connect(curv, "color", inv, "in");
                src = (inv, "float");
            }
            let thr = f("threshold");
            let w = f("softness") * 0.5 + 0.005;
            let out = place(g, "levels", 0.0, 0.0);
            g.set(out, "in_black", Value::Float(0.5 + 0.5 * (thr - w)));
            g.set(out, "in_white", Value::Float(0.5 + 0.5 * (thr + w)));
            g.connect(src.0, src.1, out, "in");
            Some((out, "float"))
        }
        "position_gradient" => {
            let grad = place(g, "gradient", -170.0, 0.0);
            g.set(grad, "axis", n.param("axis"));
            let w = f("falloff") * 0.5 + 0.005;
            g.set(grad, "start", Value::Float(0.5 - w));
            g.set(grad, "end", Value::Float(0.5 + w));
            let out = place(g, "invert", 0.0, 0.0);
            g.connect(grad, "out", out, "in");
            Some((out, "float"))
        }
        _ => None,
    };
    let (out, out_name) = result?;
    // Re-point everything that read the generator.
    let ids: Vec<NodeId> = g.nodes.keys().copied().collect();
    for k in ids {
        let node = g.node_mut(k);
        for l in node.inputs.values_mut() {
            if l.node == id {
                l.node = out;
                l.output = out_name.to_string();
            }
        }
        for m in &mut node.mask_slots {
            if m.source == MaskSource::Node(id) {
                m.source = MaskSource::Node(out);
            }
        }
    }
    g.remove(id);
    Some(out)
}

/// ID colors as baked into the ID map (matches uv_raster.wgsl), for ID Select.
pub fn id_color(i: u32) -> [f32; 3] {
    let hue = (i as f32 * 0.618034 + 0.05).fract();
    let k = [1.0, 2.0 / 3.0, 1.0 / 3.0];
    let val = 1.0 - 0.25 * ((i / 7) % 3) as f32;
    let mut c = [0.0; 3];
    for j in 0..3 {
        let p = ((hue + k[j]).fract() * 6.0 - 3.0).abs();
        c[j] = (p - 1.0).clamp(0.0, 1.0) * val;
    }
    c
}

impl NodeEditor {
    pub fn open_for(&mut self, layer: LayerRef, g: &Graph) {
        self.open = true;
        if self.layer != Some(layer) {
            self.layer = Some(layer);
            self.extra.clear();
            self.selected = None;
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
            // Lay out nodes that were never placed: right to left from the sink.
            let _ = g;
        }
        self.auto_layout_needed = true;
        self.frame_needed = true;
    }

    /// Fits all visible nodes into the canvas.
    fn frame_all(&mut self, g: &Graph, layer: LayerRef, rect: Rect) {
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        let mut nodes = self.visible(g, layer);
        nodes.push(SINK);
        for n in nodes {
            let pos = if n == SINK { self.sink_pos(g, layer) } else { g.node(n).pos };
            let h = HEADER + (inputs(g, n).len() + outputs(g, n).len()).max(1) as f32 * ROW + 6.0;
            lo = lo.min(Vec2::new(pos[0], pos[1]));
            hi = hi.max(Vec2::new(pos[0] + NODE_W, pos[1] + h));
        }
        let size = (hi - lo).max(Vec2::splat(1.0));
        self.zoom = ((rect.width() - 40.0) / size.x).min((rect.height() - 40.0) / size.y).clamp(0.3, 1.0);
        // origin = rect.center() - (NODE_W, 60); put the bounds' center at the rect's center.
        let origin = rect.center() - Vec2::new(NODE_W, 60.0);
        let center = (lo + hi) * 0.5;
        self.pan = (rect.center() - origin) / self.zoom - center;
    }

    fn visible(&self, g: &Graph, layer: LayerRef) -> Vec<NodeId> {
        let mut set = BTreeSet::new();
        if let Some(s) = ops::slot(g, layer) {
            for root in [s.content, s.mask].into_iter().flatten() {
                set.extend(g.subgraph(root));
            }
        }
        for e in &self.extra {
            if g.get(*e).is_some() {
                set.extend(g.subgraph(*e));
            }
        }
        set.into_iter().collect()
    }

    /// Places nodes at (0, 0) to the left of whatever reads them.
    fn layout(&self, g: &mut Graph, layer: LayerRef) {
        let nodes = self.visible(g, layer);
        let unplaced: Vec<NodeId> = nodes.iter().copied().filter(|n| g.node(*n).pos == [0.0, 0.0]).collect();
        if unplaced.is_empty() {
            return;
        }
        // Depth = longest path to the sink.
        let mut depth: std::collections::BTreeMap<NodeId, usize> = Default::default();
        fn walk(g: &Graph, n: NodeId, d: usize, depth: &mut std::collections::BTreeMap<NodeId, usize>) {
            if depth.get(&n).is_some_and(|x| *x >= d) || d > 64 {
                return;
            }
            depth.insert(n, d);
            for c in g.dependencies(n) {
                walk(g, c, d + 1, depth);
            }
        }
        if let Some(s) = ops::slot(g, layer) {
            for r in [s.content, s.mask].into_iter().flatten() {
                walk(g, r, 1, &mut depth);
            }
        }
        let mut rows: std::collections::BTreeMap<usize, f32> = Default::default();
        for n in unplaced {
            let d = *depth.get(&n).unwrap_or(&1);
            let y = rows.entry(d).or_insert(0.0);
            g.node_mut(n).pos = [-(d as f32) * 220.0, *y];
            *y += 140.0;
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        let Some(layer) = self.layer.filter(|l| ops::slot(&p.graph, *l).is_some()) else {
            ui.label("Select a layer and press G to edit it as a graph.");
            return;
        };
        if self.auto_layout_needed {
            self.layout(&mut p.graph, layer);
            self.auto_layout_needed = false;
        }
        let name = ops::slot(&p.graph, layer).map(|s| s.name.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.strong(format!("Node Editor: {name}"));
            ui.label(egui::RichText::new("Tab: add node · drag outputs to inputs · Delete: remove · middle drag: pan · wheel: zoom · Home: frame all · Esc: close").weak());
        });
        egui::Panel::right("node props").resizable(true).default_size(260.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.properties(ui, gpu, p));
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui, gpu, p, layer));
    }

    fn to_screen(&self, origin: Pos2, pos: [f32; 2]) -> Pos2 {
        origin + (Vec2::new(pos[0], pos[1]) + self.pan) * self.zoom
    }

    fn sink_pos(&self, g: &Graph, layer: LayerRef) -> [f32; 2] {
        let content = ops::slot(g, layer).and_then(|s| s.content).and_then(|c| g.get(c));
        let x = content.map(|n| n.pos[0] + 240.0).unwrap_or(0.0).max(0.0);
        [x, content.map(|n| n.pos[1]).unwrap_or(0.0)]
    }

    fn node_rect(&self, g: &Graph, origin: Pos2, id: NodeId, layer: LayerRef) -> Rect {
        let pos = if id == SINK { self.sink_pos(g, layer) } else { g.node(id).pos };
        let rows = inputs(g, id).len() + outputs(g, id).len();
        let h = HEADER + rows.max(1) as f32 * ROW + 6.0;
        Rect::from_min_size(self.to_screen(origin, pos), Vec2::new(NODE_W, h) * self.zoom)
    }

    fn input_pos(&self, g: &Graph, origin: Pos2, id: NodeId, name: &str, layer: LayerRef) -> Option<Pos2> {
        let r = self.node_rect(g, origin, id, layer);
        let i = inputs(g, id).iter().position(|(n, _, _)| n == name)?;
        let row = outputs(g, id).len() + i;
        Some(Pos2::new(r.left(), r.top() + (HEADER + ROW * (row as f32 + 0.5)) * self.zoom))
    }

    fn output_pos(&self, g: &Graph, origin: Pos2, id: NodeId, name: &str, layer: LayerRef) -> Option<Pos2> {
        let r = self.node_rect(g, origin, id, layer);
        let i = outputs(g, id).iter().position(|(n, _)| n == name)?;
        Some(Pos2::new(r.right(), r.top() + (HEADER + ROW * (i as f32 + 0.5)) * self.zoom))
    }

    fn canvas(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, layer: LayerRef) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(28));
        if self.frame_needed || (resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Home))) {
            self.frame_all(&p.graph, layer, rect);
            self.frame_needed = false;
        }
        let origin = rect.center() - Vec2::new(NODE_W, 60.0);
        let pointer = ui.input(|i| i.pointer.hover_pos());

        // Pan and zoom.
        let (middle, scroll) = ui.input(|i| (i.pointer.middle_down(), i.smooth_scroll_delta.y));
        if resp.hovered() && scroll != 0.0 {
            let z = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.3, 2.5);
            if let Some(ptr) = pointer {
                // Keep the point under the cursor fixed.
                let canvas = (ptr - origin) / self.zoom - self.pan;
                self.pan = (ptr - origin) / z - canvas;
            }
            self.zoom = z;
        }
        if middle && resp.dragged() {
            self.pan += resp.drag_delta() / self.zoom;
        }

        let nodes = self.visible(&p.graph, layer);
        let mut all = nodes.clone();
        all.push(SINK);

        // Links.
        for &n in &all {
            for (input, src, out) in links_into(&p.graph, layer, n) {
                if !all.contains(&src) {
                    continue;
                }
                let (Some(a), Some(b)) = (
                    self.output_pos(&p.graph, origin, src, &out, layer),
                    self.input_pos(&p.graph, origin, n, &input, layer),
                ) else {
                    continue;
                };
                let ty = p.graph.output_type(&graph::Link { node: src, output: out.clone() }).unwrap_or(SocketType::Float);
                draw_link(&painter, a, b, type_color(ty), self.zoom);
            }
        }

        // Nodes.
        let mut clicked_socket: Option<(SocketRef, bool, Pos2)> = None;
        let font = egui::FontId::proportional(12.0 * self.zoom);
        for &n in &all {
            let r = self.node_rect(&p.graph, origin, n, layer);
            let selected = self.selected == Some(n);
            let (title, cached, error) = if n == SINK {
                (format!("Layer: {}", ops::slot(&p.graph, layer).map(|s| s.name.as_str()).unwrap_or("")), false, false)
            } else {
                let node = p.graph.node(n);
                (node.title().to_string(), node.cached, false)
            };
            painter.rect_filled(r, 5.0, Color32::from_gray(if n == SINK { 50 } else { 42 }));
            let header = Rect::from_min_size(r.min, Vec2::new(r.width(), HEADER * self.zoom));
            painter.rect_filled(header, 5.0, if n == SINK { Color32::from_rgb(60, 80, 120) } else { Color32::from_rgb(60, 60, 75) });
            painter.text(header.left_center() + Vec2::new(6.0, 0.0), egui::Align2::LEFT_CENTER, &title, font.clone(), Color32::WHITE);
            if cached {
                painter.text(header.right_center() - Vec2::new(6.0, 0.0), egui::Align2::RIGHT_CENTER, "cached", egui::FontId::proportional(10.0 * self.zoom), Color32::from_rgb(120, 200, 230));
            }
            let _ = error;
            painter.rect_stroke(r, 5.0, Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { Color32::from_rgb(240, 190, 60) } else { Color32::from_gray(70) }), egui::StrokeKind::Inside);
            for (name, ty) in outputs(&p.graph, n) {
                let pos = self.output_pos(&p.graph, origin, n, &name, layer).unwrap();
                painter.circle_filled(pos, 5.0 * self.zoom, type_color(ty));
                painter.text(pos - Vec2::new(10.0 * self.zoom, 0.0), egui::Align2::RIGHT_CENTER, pretty(&name), font.clone(), Color32::LIGHT_GRAY);
                if pointer.is_some_and(|pt| pt.distance(pos) < 9.0 * self.zoom) && ui.input(|i| i.pointer.primary_pressed()) {
                    clicked_socket = Some((SocketRef { node: n, name: name.clone() }, true, pos));
                }
            }
            for (name, label, ty) in inputs(&p.graph, n) {
                let pos = self.input_pos(&p.graph, origin, n, &name, layer).unwrap();
                painter.circle_filled(pos, 5.0 * self.zoom, type_color(ty));
                painter.text(pos + Vec2::new(10.0 * self.zoom, 0.0), egui::Align2::LEFT_CENTER, label, font.clone(), Color32::LIGHT_GRAY);
                if pointer.is_some_and(|pt| pt.distance(pos) < 9.0 * self.zoom) && ui.input(|i| i.pointer.primary_pressed()) {
                    clicked_socket = Some((SocketRef { node: n, name: name.clone() }, false, pos));
                }
            }
        }

        // Start drags.
        if ui.input(|i| i.pointer.primary_pressed()) && resp.hovered() && self.drag.is_none() {
            if let Some((sock, is_output, _)) = clicked_socket {
                if is_output {
                    let ty = p.graph.output_type(&graph::Link { node: sock.node, output: sock.name.clone() }).unwrap_or(SocketType::Float);
                    self.drag = Some(Drag::Link { from: sock, ty });
                } else if let Some((_, src, out)) = links_into(&p.graph, layer, sock.node).into_iter().find(|(i, _, _)| *i == sock.name) {
                    // Pick up an existing link from its input end.
                    p.checkpoint("Disconnect");
                    disconnect(&mut p.graph, layer, &sock);
                    p.graph_changed(gpu);
                    let ty = p.graph.output_type(&graph::Link { node: src, output: out.clone() }).unwrap_or(SocketType::Float);
                    self.drag = Some(Drag::Link { from: SocketRef { node: src, name: out }, ty });
                }
            } else if let Some(ptr) = pointer {
                let hit = all.iter().rev().copied().find(|n| self.node_rect(&p.graph, origin, *n, layer).contains(ptr));
                self.selected = hit;
                match hit {
                    Some(n) if n != SINK => {
                        p.checkpoint("Move node");
                        self.drag = Some(Drag::Node(n));
                    }
                    _ => self.drag = Some(Drag::Pan),
                }
            }
        }
        if let Some(drag) = self.drag.clone() {
            let delta = ui.input(|i| i.pointer.delta()) / self.zoom;
            match drag {
                Drag::Node(n) => {
                    let pos = &mut p.graph.node_mut(n).pos;
                    pos[0] += delta.x;
                    pos[1] += delta.y;
                }
                Drag::Pan => self.pan += delta,
                Drag::Link { ref from, ty } => {
                    if let (Some(a), Some(b)) = (self.output_pos(&p.graph, origin, from.node, &from.name, layer), pointer) {
                        draw_link(&painter, a, b, type_color(ty), self.zoom);
                    }
                }
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                if let Drag::Link { from, .. } = &drag {
                    // Drop on the nearest input socket.
                    let target = pointer.and_then(|pt| {
                        all.iter().find_map(|&n| {
                            inputs(&p.graph, n).into_iter().find_map(|(name, _, _)| {
                                let pos = self.input_pos(&p.graph, origin, n, &name, layer)?;
                                (pos.distance(pt) < 12.0 * self.zoom).then(|| SocketRef { node: n, name })
                            })
                        })
                    });
                    if let Some(to) = target {
                        let mut g2 = p.graph.clone();
                        if connect(&mut g2, layer, from, &to) {
                            p.checkpoint("Connect");
                            p.graph = g2;
                            self.extra.remove(&from.node);
                            p.graph_changed(gpu);
                        }
                    }
                }
                self.drag = None;
            }
        }

        // Keyboard: Tab adds a node, Delete removes the selected node.
        let (tab, del) = ui.input(|i| (i.key_pressed(egui::Key::Tab), i.key_pressed(egui::Key::Delete)));
        if resp.hovered() && tab {
            let at = pointer.unwrap_or(rect.center());
            let canvas = (at - origin) / self.zoom - self.pan;
            self.add_menu = Some((Pos2::new(canvas.x, canvas.y), String::new()));
        }
        if resp.hovered() && del {
            if let Some(n) = self.selected.filter(|n| *n != SINK) {
                let ty = p.graph.node(n).ty.clone();
                if ty != "material_output" && n != p.graph.root {
                    p.edit(gpu, "Delete node", |g| g.remove(n));
                    self.selected = None;
                    self.extra.remove(&n);
                }
            }
        }
        if resp.secondary_clicked() {
            let at = pointer.unwrap_or(rect.center());
            let canvas = (at - origin) / self.zoom - self.pan;
            self.add_menu = Some((Pos2::new(canvas.x, canvas.y), String::new()));
        }
        self.add_menu_ui(ui, gpu, p, origin);
    }

    fn add_menu_ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, origin: Pos2) {
        let Some((at, mut search)) = self.add_menu.take() else { return };
        let screen = self.to_screen(origin, [at.x, at.y]);
        let mut keep = true;
        let mut chosen = None;
        egui::Area::new(egui::Id::new("add node")).fixed_pos(screen).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(200.0);
                let r = ui.text_edit_singleline(&mut search);
                r.request_focus();
                let q = search.to_ascii_lowercase();
                let mut first = None;
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    let mut category = "";
                    for d in graph::defs() {
                        if matches!(d.ty, "material_output" | "stack") {
                            continue;
                        }
                        if !q.is_empty() && !d.title.to_ascii_lowercase().contains(&q) {
                            continue;
                        }
                        if d.category != category {
                            category = d.category;
                            ui.label(egui::RichText::new(category).weak().small());
                        }
                        first.get_or_insert(d.ty);
                        if ui.button(d.title).clicked() {
                            chosen = Some(d.ty);
                        }
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    chosen = first;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
        if let Some(ty) = chosen {
            let id = p.edit(gpu, "Add node", |g| {
                let id = g.add(ty);
                g.node_mut(id).pos = [at.x, at.y];
                id
            });
            self.extra.insert(id);
            self.selected = Some(id);
            keep = false;
        }
        if keep {
            self.add_menu = Some((at, search));
        }
    }

    fn properties(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        let Some(n) = self.selected.filter(|n| *n != SINK && p.graph.get(*n).is_some()) else {
            ui.label(egui::RichText::new("Select a node.").weak());
            return;
        };
        let node = p.graph.node(n).clone();
        let def = node.def();
        ui.heading(node.title());
        let mut label = node.label.clone();
        if ui.add(egui::TextEdit::singleline(&mut label).hint_text("Label")).lost_focus() && label != node.label {
            p.edit(gpu, "Rename node", |g| g.node_mut(n).label = label);
        }
        let mut cached = node.cached;
        let r = ui.add_enabled(!def.non_local, egui::Checkbox::new(&mut cached, "Cached"));
        if r.changed() {
            p.edit(gpu, "Cached", |g| g.node_mut(n).cached = cached);
        }
        if def.non_local {
            ui.label(egui::RichText::new("Filters read neighboring texels and are always cached.").weak().small());
        }
        if crate::graph::GENERATORS.contains(&node.ty.as_str()) && ui.button("Ungroup").on_hover_text("Replace with its component nodes").clicked() {
            if let Some(out) = p.edit(gpu, "Ungroup", |g| ungroup(g, n)) {
                self.selected = Some(out);
                self.auto_layout_needed = true;
            }
            return;
        }
        ui.separator();
        for pd in &def.params {
            match pd.kind {
                graph::ParamKind::Stops => self.stops_editor(ui, gpu, p, n),
                graph::ParamKind::Colors => self.id_editor(ui, gpu, p, n),
                _ => crate::ui_layers::param_widget(ui, gpu, p, n, pd, pd.name),
            }
        }
    }

    fn stops_editor(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, n: NodeId) {
        let Value::Stops(mut stops) = p.graph.node(n).param("stops") else { return };
        ui.label("Stops");
        let mut changed = false;
        let mut remove = None;
        for (i, (pos, c)) in stops.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(pos).range(0.0..=1.0).speed(0.005)).changed();
                changed |= ui.color_edit_button_rgb(c).changed();
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove.filter(|_| stops.len() > 1) {
            stops.remove(i);
            changed = true;
        }
        if stops.len() < 8 && ui.button("+ Stop").clicked() {
            stops.push((1.0, [1.0; 3]));
            changed = true;
        }
        if changed {
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            p.edit(gpu, "Edit ramp", |g| g.set(n, "stops", Value::Stops(stops)));
        }
    }

    fn id_editor(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, n: NodeId) {
        let Value::Colors(colors) = p.graph.node(n).param("colors") else { return };
        ui.label("Selected IDs (mesh materials)");
        let mut new = colors.clone();
        for (i, name) in p.mesh.materials.clone().iter().enumerate() {
            let c = id_color(i as u32);
            let mut on = colors.iter().any(|x| (x[0] - c[0]).abs() + (x[1] - c[1]).abs() + (x[2] - c[2]).abs() < 1e-3);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
                if ui.checkbox(&mut on, name).changed() {
                    new.retain(|x| (x[0] - c[0]).abs() + (x[1] - c[1]).abs() + (x[2] - c[2]).abs() >= 1e-3);
                    if on {
                        new.push(c);
                    }
                }
            });
        }
        if new != colors {
            p.edit(gpu, "Select IDs", |g| g.set(n, "colors", Value::Colors(new)));
        }
    }
}

fn draw_link(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, zoom: f32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    painter.add(CubicBezierShape::from_points_stroke(
        [a, a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new(2.0 * zoom, color),
    ));
}
