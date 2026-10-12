//! Node Editor. See docs/reference/nodes.md and docs/how-to/edit-a-layer-as-a-graph.md.
//!
//! Shows either the subgraph behind one layer (its content and mask, plus a virtual "Layer"
//! sink node whose inputs are the layer's content and mask) or, with no layer in focus, the
//! whole project graph. Connecting into the sink, into a Stack's layer sockets or into a Mask
//! Stack's sockets edits those slots; the "+ New layer" / "+ New effect" sockets add slots.
//!
//! Nodes show their outputs, then their parameters (edited in place), then their inputs, and
//! can be collapsed to their header. See docs/reference/node-editor.md "Nodes".
//!
//! Every layer of a Stack is drawn as one virtual "layer group" node (an id with GROUP_BIT set,
//! encoding the slot) instead of the nodes behind it. See docs/reference/node-editor.md
//! "Layer groups". Frames (Graph::frames) are selected by ids with FRAME_BIT set; see "Frames".

use std::collections::BTreeSet;

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2, epaint::CubicBezierShape};

use crate::gpu::Gpu;
use crate::graph::{self, Graph, MaskSlot, MaskSource, NodeId, Slot, SocketType, Value};
use crate::ops::{self, LayerRef};
use crate::project::Project;

mod wrangler;
use wrangler::{Edge, MergeOp};

const SINK: NodeId = u32::MAX;
/// Layer group ids: this bit, the Stack's id in bits 12..31 and the slot index in bits 0..12.
const GROUP_BIT: NodeId = 1 << 31;
/// Frame ids: this bit and the frame's index in Graph::frames.
const FRAME_BIT: NodeId = 1 << 30;
const NODE_W: f32 = 200.0;
/// A Reroute's pill (docs/reference/node-editor.md "Reroutes").
const REROUTE: Vec2 = Vec2::new(32.0, 18.0);
/// Space around a frame's nodes.
const FRAME_PAD: f32 = 16.0;
/// Below this zoom, parameters are shown as text and can't be edited.
const EDIT_ZOOM: f32 = 0.5;
/// Header and row height: a control (ui_theme::H_CONTROL) and a gap.
const HEADER: f32 = 26.0;
const ROW: f32 = 26.0;
/// Header color of layer groups (docs/reference/node-editor.md "Layer groups").
const GROUP_COLOR: Color32 = Color32::from_rgb(0x3b, 0x59, 0x59);
/// The editor's inputs, listed under "? All shortcuts" (docs/reference/node-editor.md).
const SHORTCUTS: &[(&str, &str)] = &[
    ("Tab, right-click", "Add menu"),
    ("Double-click a layer group", "Open the layer"),
    ("Drag from output", "Connect"),
    ("Drag empty space", "Box select (Shift adds)"),
    ("Right-drag, Alt+drag", "Pan"),
    ("Wheel", "Zoom"),
    ("Ctrl+Shift+Click", "Preview a node; on empty space, stop"),
    ("Alt+right-drag", "Lazy connect"),
    ("Shift+Alt+right-drag", "Lazy connect with a menu"),
    ("Ctrl+right-drag", "Cut links"),
    ("Shift+right-drag", "Add reroutes on the links crossed"),
    ("F, Alt+P", "Frame selected nodes, take them out of frames"),
    ("Right-click a node", "Node menu"),
    ("Delete", "Delete selected"),
    ("Ctrl+X", "Dissolve selected"),
    ("Alt+X", "Delete unused nodes"),
    ("Ctrl+C, Ctrl+V", "Copy, paste"),
    ("H", "Collapse or expand"),
    ("Shift+W A S D", "Align top, left, bottom, right"),
    ("Shift+=", "Align in a row or column"),
    ("Alt+S", "Swap"),
    ("K", "Link active to selected"),
    ("Shift+C", "Copy settings to selected"),
    ("Backspace", "Reset to defaults"),
    ("A, Alt+A", "Select all, none"),
    ("Home", "Frame all"),
    ("Ctrl+0 + - 8 / , .", "Merge: mix, add, subtract, multiply, divide, min, max"),
];
const NEW_LAYER: &str = "slot_new";
const NEW_EFFECT: &str = "m_new";

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocketRef {
    node: NodeId,
    name: String,
}

#[derive(Clone, Debug)]
enum Drag {
    /// Moving the selected nodes.
    Nodes,
    /// Dragging a new link from an output.
    Link { from: SocketRef, ty: SocketType },
    Pan,
    /// Box selection, from this screen position.
    Box { start: Pos2, additive: bool },
    /// Ctrl + right drag: cuts the links the stroke crosses.
    Cut { points: Vec<Pos2> },
    /// Shift + right drag: puts reroutes on the links the stroke crosses.
    Reroute { points: Vec<Pos2> },
    /// Alt + right drag from one node to another: connects them (with Shift: choose sockets).
    Lazy { from: NodeId, menu: bool },
}

/// Node Wrangler operations, from shortcuts or the node context menu.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Merge(MergeOp),
    Swap,
    Dissolve,
    DeleteUnused,
    Duplicate,
    Align,
    AlignEdges(Edge),
    ToggleCollapse,
    Copy,
    Reset,
    CopySettings,
    LinkToSelected,
    SelectAll,
    DeselectAll,
    Frame,
    Unframe,
}

/// What the editor shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// The whole project graph.
    Root,
    /// One layer's content and mask, with the Layer sink.
    Layer(LayerRef),
}

/// The add-node menu, optionally connecting the new node to a dragged link.
struct AddMenu {
    /// Canvas position for the new node.
    at: Pos2,
    search: String,
    link: Option<(SocketRef, SocketType)>,
}

pub struct NodeEditor {
    pub open: bool,
    /// Screen area of the editor last frame, so global shortcuts can yield to it.
    pub rect: Rect,
    pub focus: Focus,
    pan: Vec2,
    zoom: f32,
    selected: BTreeSet<NodeId>,
    /// The node whose properties are shown (the last one clicked).
    primary: Option<NodeId>,
    drag: Option<Drag>,
    /// Whether the current node drag has moved anything (for its undo checkpoint).
    moved: bool,
    /// Nodes added in this editor session that are not yet connected.
    extra: BTreeSet<NodeId>,
    add_menu: Option<AddMenu>,
    /// Right-click menu of a node (screen position).
    node_menu: Option<Pos2>,
    /// Socket chooser after a Shift+Alt lazy connect: (screen position, from, to).
    lazy_menu: Option<(Pos2, NodeId, NodeId)>,
    /// Ramp or ID list editor opened from a parameter row: (node, parameter, screen position).
    param_popup: Option<(NodeId, &'static str, Pos2)>,
    /// Editable parameter rows this frame (screen), so presses on them don't drag the node.
    param_rects: Vec<(NodeId, Rect)>,
    auto_layout_needed: bool,
    frame_needed: bool,
    /// Tab was pressed over the canvas this frame (taken before any widget saw it).
    tab_pressed: bool,
    /// Screen area of the canvas last frame.
    canvas_rect: Rect,
    /// The frames' members at the end of last frame: when they change elsewhere (undo, nodes
    /// deleted), frame ids in the selection may point at other frames and are dropped.
    frames_seen: Vec<BTreeSet<NodeId>>,
}

impl Default for NodeEditor {
    fn default() -> Self {
        Self {
            open: false,
            rect: Rect::NOTHING,
            focus: Focus::Root,
            pan: Vec2::ZERO,
            zoom: 1.0,
            selected: BTreeSet::new(),
            primary: None,
            drag: None,
            moved: false,
            extra: BTreeSet::new(),
            add_menu: None,
            node_menu: None,
            lazy_menu: None,
            param_popup: None,
            param_rects: Vec::new(),
            auto_layout_needed: false,
            frame_needed: false,
            tab_pressed: false,
            canvas_rect: Rect::NOTHING,
            frames_seen: Vec::new(),
        }
    }
}

fn type_color(t: SocketType) -> Color32 {
    match t {
        SocketType::Float => crate::ui_theme::SOCK_FLOAT,
        SocketType::Color => crate::ui_theme::SOCK_COLOR,
        SocketType::Vector => crate::ui_theme::SOCK_VECTOR,
        SocketType::Material => crate::ui_theme::SOCK_MATERIAL,
    }
}

/// Float, Color and Vector convert into each other; Material only connects to Material.
fn compatible(from: SocketType, to: SocketType) -> bool {
    from == to || (from != SocketType::Material && to != SocketType::Material)
}

/// The layer group node of a Stack slot, if the slot fits the id encoding (docs/reference/limits.md).
fn group_id(l: LayerRef) -> Option<NodeId> {
    (l.stack < 1 << 19 && l.index < 1 << 12).then_some(GROUP_BIT | l.stack << 12 | l.index as NodeId)
}

/// The layer a layer group node shows.
fn group_of(id: NodeId) -> Option<LayerRef> {
    (id != SINK && id & GROUP_BIT != 0).then_some(LayerRef { stack: (id & !GROUP_BIT) >> 12, index: (id & 0xfff) as usize })
}

/// The sink, layer groups and frames: drawn and selected, but not nodes of the graph.
fn is_virtual(id: NodeId) -> bool {
    id == SINK || id & (GROUP_BIT | FRAME_BIT) != 0
}

fn frame_of(id: NodeId) -> Option<usize> {
    (id != SINK && id & GROUP_BIT == 0 && id & FRAME_BIT != 0).then_some((id & !FRAME_BIT) as usize)
}

fn frame_id(index: usize) -> NodeId {
    FRAME_BIT | index as NodeId
}

fn is_reroute(g: &Graph, id: NodeId) -> bool {
    !is_virtual(id) && g.get(id).is_some_and(|n| n.ty == "reroute")
}

/// Whether input `to` of `node` (of type `to_ty`) takes a link of type `from`: a Reroute takes
/// any type.
fn accepts(g: &Graph, node: NodeId, from: SocketType, to_ty: SocketType) -> bool {
    compatible(from, to_ty) || is_reroute(g, node)
}

fn node_width(g: &Graph, id: NodeId) -> f32 {
    if is_reroute(g, id) { REROUTE.x } else { NODE_W }
}

/// A layer group's slot, if the layer still exists.
fn group_slot(g: &Graph, id: NodeId) -> Option<&Slot> {
    group_of(id).and_then(|l| ops::slot(g, l))
}

/// Input sockets of a node, including slot sockets of stacks.
fn inputs(g: &Graph, id: NodeId) -> Vec<(String, String, SocketType)> {
    if id == SINK {
        return vec![
            ("content".into(), "Content".into(), SocketType::Material),
            ("mask".into(), "Mask".into(), SocketType::Float),
        ];
    }
    if is_virtual(id) {
        return vec![];
    }
    let n = g.node(id);
    if n.ty == "reroute" {
        let t = g.output_type(&graph::Link { node: id, output: "out".into() }).unwrap_or(SocketType::Float);
        return vec![("in".into(), String::new(), t)];
    }
    let mut v: Vec<_> = n.def().input_sockets().into_iter().map(|(name, t)| (name.to_string(), pretty(name), t)).collect();
    for (i, s) in n.slots.iter().enumerate() {
        v.push((format!("slot{i}"), s.name.clone(), SocketType::Material));
        v.push((format!("slot{i}.mask"), format!("{} mask", s.name), SocketType::Float));
    }
    if n.ty == "stack" {
        v.push((NEW_LAYER.into(), "+ New layer".into(), SocketType::Material));
    }
    for (i, m) in n.mask_slots.iter().enumerate() {
        if matches!(m.source, MaskSource::Node(_)) {
            v.push((format!("m{i}"), m.name.clone(), SocketType::Float));
        }
    }
    if n.ty == "mask_stack" {
        v.push((NEW_EFFECT.into(), "+ New effect".into(), SocketType::Float));
    }
    v
}

fn outputs(g: &Graph, id: NodeId) -> Vec<(String, SocketType)> {
    if id == SINK {
        return vec![];
    }
    if is_virtual(id) {
        let Some(s) = group_slot(g, id) else { return vec![] };
        let mut v = Vec::new();
        if s.content.is_some_and(|c| g.get(c).is_some()) {
            v.push(("content".to_string(), SocketType::Material));
        }
        if s.mask.is_some_and(|m| g.get(m).is_some()) {
            v.push(("mask".to_string(), SocketType::Float));
        }
        return v;
    }
    // Through Graph::output_type, for the types that follow a node's inputs or settings.
    let ty = |o: &str| g.output_type(&graph::Link { node: id, output: o.to_string() });
    g.node(id).def().outputs.iter().map(|(n, t)| (n.to_string(), ty(n).unwrap_or(*t))).collect()
}

fn pretty(s: &str) -> String {
    let mut out = s.replace('_', " ");
    if let Some(f) = out.get(0..1) {
        out.replace_range(0..1, &f.to_uppercase());
    }
    out
}

/// Rows a parameter takes: a Vector its name and one row per component, others one.
fn param_span(pd: &graph::ParamDef) -> usize {
    if pd.kind == graph::ParamKind::Vec3 { 4 } else { 1 }
}

/// The parameters shown as parameter rows, not on an input's row.
fn own_params(g: &Graph, id: NodeId) -> Vec<&'static graph::ParamDef> {
    if is_virtual(id) {
        return vec![];
    }
    let n = g.node(id);
    n.def().params.iter().filter(|p| !p.socket && n.shows(p)).collect()
}

/// The parameter whose field an input shows while unconnected.
fn input_value(g: &Graph, id: NodeId, input: &str) -> Option<&'static graph::ParamDef> {
    if is_virtual(id) {
        return None;
    }
    let n = g.node(id);
    n.def().input_param(input).filter(|_| !n.inputs.contains_key(input))
}

/// The value an input has of its own, connected or not (None for link-only inputs).
fn input_value_def(g: &Graph, id: NodeId, input: &str) -> Option<&'static graph::ParamDef> {
    if is_virtual(id) {
        return None;
    }
    g.node(id).def().input_param(input)
}

/// Rows an input takes: its value's rows while unconnected, otherwise one.
fn input_span(g: &Graph, id: NodeId, input: &str) -> usize {
    input_value(g, id, input).map_or(1, param_span)
}

fn param_rows(g: &Graph, id: NodeId) -> usize {
    own_params(g, id).iter().map(|p| param_span(p)).sum()
}

/// The row an input starts on (0 = first row below the header).
fn input_row(g: &Graph, id: NodeId, input: &str) -> Option<usize> {
    let mut row = outputs(g, id).len() + param_rows(g, id);
    for (name, _, _) in inputs(g, id) {
        if name == input {
            return Some(row);
        }
        row += input_span(g, id, &name);
    }
    None
}

fn is_collapsed(g: &Graph, id: NodeId) -> bool {
    !is_virtual(id) && g.node(id).collapsed && g.node(id).ty != "reroute"
}

/// Height in canvas units: the header, then the rows of outputs, parameters and inputs.
fn node_height(g: &Graph, id: NodeId) -> f32 {
    if is_reroute(g, id) {
        return REROUTE.y;
    }
    if is_collapsed(g, id) {
        return HEADER;
    }
    let ins: usize = inputs(g, id).iter().map(|(name, _, _)| input_span(g, id, name)).sum();
    HEADER + (outputs(g, id).len() + param_rows(g, id) + ins).max(1) as f32 * ROW + 6.0
}

/// Header color: the category's, the group color, or Output for the Layer sink.
fn header_color(g: &Graph, id: NodeId) -> Color32 {
    let [r, gr, b] = if id == SINK {
        graph::Category::Output.color()
    } else if is_virtual(id) {
        return GROUP_COLOR;
    } else {
        g.node(id).def().category.color()
    };
    Color32::from_rgb(r, gr, b)
}

/// The default output used when a node is referenced without an output name (slots).
fn main_output(g: &Graph, id: NodeId) -> String {
    g.node(id).def().outputs.first().map(|(n, _)| n.to_string()).unwrap_or_else(|| "out".into())
}

/// All links into `id`: (input socket, source node, source output).
fn links_into(g: &Graph, layer: Option<LayerRef>, id: NodeId) -> Vec<(String, NodeId, String)> {
    let mut v = Vec::new();
    if is_virtual(id) && id != SINK {
        return v;
    }
    if id == SINK {
        if let Some(s) = layer.and_then(|l| ops::slot(g, l)) {
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

/// The links drawn into `id`: as `links_into`, but a Stack's layers come from their groups.
fn shown_links(g: &Graph, layer: Option<LayerRef>, id: NodeId) -> Vec<(String, NodeId, String)> {
    let mut v = links_into(g, layer, id);
    if id != SINK && g.get(id).is_some_and(|n| n.ty == "stack") {
        for (input, src, out) in &mut v {
            let Some(rest) = input.strip_prefix("slot") else { continue };
            let (idx, mask) = match rest.strip_suffix(".mask") {
                Some(i) => (i, true),
                None => (rest, false),
            };
            let Ok(index) = idx.parse::<usize>() else { continue };
            if let Some(gid) = group_id(LayerRef { stack: id, index }) {
                *src = gid;
                *out = if mask { "mask" } else { "content" }.into();
            }
        }
    }
    v
}

/// The nodes and layer groups shown from `start`: everything it reads, but a Stack's layers
/// as their groups instead of the nodes behind them.
fn shown_from(g: &Graph, start: NodeId, out: &mut BTreeSet<NodeId>) {
    let mut todo = vec![start];
    while let Some(n) = todo.pop() {
        let Some(node) = g.get(n) else { continue };
        if !out.insert(n) {
            continue;
        }
        todo.extend(node.inputs.values().map(|l| l.node));
        for m in &node.mask_slots {
            if let MaskSource::Node(c) = m.source {
                todo.push(c);
            }
        }
        for (index, s) in node.slots.iter().enumerate() {
            match group_id(LayerRef { stack: n, index }) {
                Some(gid) => {
                    out.insert(gid);
                }
                None => todo.extend(s.content.into_iter().chain(s.mask)),
            }
        }
    }
}

/// Deletes frames by index (not their nodes).
fn remove_frames(g: &mut Graph, frames: &BTreeSet<usize>) {
    let mut i = 0;
    g.frames.retain(|_| {
        i += 1;
        !frames.contains(&(i - 1))
    });
}

/// Moves a node or a layer group (the sink isn't stored).
fn set_pos(g: &mut Graph, id: NodeId, pos: [f32; 2]) {
    if id == SINK {
        return;
    }
    match group_of(id) {
        Some(l) => {
            if let Some(s) = ops::slot_mut(g, l) {
                s.node_pos = pos;
            }
        }
        None => {
            if let Some(n) = g.nodes.get_mut(&id) {
                n.pos = pos;
            }
        }
    }
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
fn connect(g: &mut Graph, layer: Option<LayerRef>, from: &SocketRef, to: &SocketRef) -> bool {
    let Some(from_ty) = g.get(from.node).and_then(|_| g.output_type(&graph::Link { node: from.node, output: from.name.clone() })) else {
        return false;
    };
    let to_ty = match inputs(g, to.node).into_iter().find(|(n, _, _)| *n == to.name) {
        Some((_, _, t)) => t,
        None => return false,
    };
    if !accepts(g, to.node, from_ty, to_ty) {
        return false;
    }
    let would_cycle = |g: &Graph, target: NodeId| target == from.node || g.depends_on(from.node, target);
    if to.node == SINK {
        let Some(layer) = layer else { return false };
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
    if to.name == NEW_LAYER {
        let name = g.node(from.node).title().to_string();
        g.node_mut(to.node).slots.push(Slot::new(name, from.node));
        return true;
    }
    if to.name == NEW_EFFECT {
        let name = g.node(from.node).title().to_string();
        g.node_mut(to.node).mask_slots.push(MaskSlot {
            name,
            visible: true,
            opacity: 1.0,
            mode: graph::BlendMode::Normal,
            source: MaskSource::Node(from.node),
        });
        return true;
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

fn disconnect(g: &mut Graph, layer: Option<LayerRef>, to: &SocketRef) {
    if to.node == SINK {
        if let Some(s) = layer.and_then(|l| ops::slot_mut(g, l)) {
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
    let place = |g: &mut Graph, ty: &str, dx: f32, dy: f32| {
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
            g.set(scale, "b", Value::Float(2.0 * na));
            g.connect(nlev, "float", scale, "a");
            let offset = place(g, "math", 0.0, 140.0);
            g.set(offset, "op", Value::Text("Add".into()));
            g.set(offset, "b", Value::Float(1.0 - na));
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
    /// Opens the editor on one layer's graph.
    pub fn open_for(&mut self, layer: LayerRef) {
        self.set_focus(Focus::Layer(layer));
        self.open = true;
    }

    /// Opens the editor on the whole project graph.
    pub fn open_root(&mut self) {
        self.set_focus(Focus::Root);
        self.open = true;
    }

    /// Ctrl+Space: closes the editor, or opens it on the selected layer (or the whole graph).
    pub fn toggle(&mut self, layer: Option<LayerRef>) {
        if self.open {
            self.open = false;
        } else {
            match layer {
                Some(l) => self.open_for(l),
                None => self.open_root(),
            }
        }
    }

    /// Tab over the canvas opens the add menu. egui moves the keyboard focus to the next control
    /// on Tab, and a focused control then keeps the editor's keys from working, so this runs
    /// before any widget: it takes the key and cancels the focus move. Typing in a text field
    /// keeps its Tab. See docs/reference/node-editor.md "Keys".
    pub fn intercept_tab(&mut self, ctx: &egui::Context) {
        if !self.open || ctx.text_edit_focused() || self.has_popup() {
            return;
        }
        // Not under a dropdown, menu or window over the canvas: those keep their keys.
        let pos = ctx.input(|i| i.pointer.hover_pos());
        let covered = pos.and_then(|p| ctx.layer_id_at(p)).is_some_and(|l| l.order != egui::Order::Background);
        let over = pos.is_some_and(|p| self.canvas_rect.contains(p)) && !covered && !egui::Popup::is_any_open(ctx);
        if over && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
            ctx.memory_mut(|m| {
                m.move_focus(egui::FocusDirection::None);
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            self.tab_pressed = true;
        }
    }

    /// Whether one of the editor's menus is open (Esc closes it rather than the editor).
    pub fn has_popup(&self) -> bool {
        self.add_menu.is_some() || self.node_menu.is_some() || self.lazy_menu.is_some() || self.param_popup.is_some()
    }

    fn set_focus(&mut self, focus: Focus) {
        if self.focus != focus {
            self.focus = focus;
            self.extra.clear();
            self.selected.clear();
            self.primary = None;
            self.add_menu = None;
            self.param_popup = None;
            self.drag = None;
        }
        self.auto_layout_needed = true;
        self.frame_needed = true;
    }

    fn layer(&self) -> Option<LayerRef> {
        match self.focus {
            Focus::Layer(l) => Some(l),
            Focus::Root => None,
        }
    }

    /// Nodes and layer groups shown, without the sink.
    fn visible(&self, g: &Graph) -> Vec<NodeId> {
        let mut set = BTreeSet::new();
        match self.focus {
            Focus::Layer(l) => {
                if let Some(s) = ops::slot(g, l) {
                    for root in [s.content, s.mask].into_iter().flatten() {
                        shown_from(g, root, &mut set);
                    }
                }
            }
            Focus::Root => {
                if let Some(out) = g.output_node() {
                    shown_from(g, out, &mut set);
                }
            }
        }
        for e in &self.extra {
            shown_from(g, *e, &mut set);
        }
        set.into_iter().collect()
    }

    /// Nodes shown, with the sink when a layer is in focus.
    fn all_nodes(&self, g: &Graph) -> Vec<NodeId> {
        let mut v = self.visible(g);
        if self.layer().is_some() {
            v.push(SINK);
        }
        v
    }

    /// Fits all visible nodes into the canvas.
    fn frame_all(&mut self, g: &Graph, rect: Rect) {
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        let nodes = self.all_nodes(g);
        if nodes.is_empty() {
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
            return;
        }
        for n in nodes {
            let pos = self.node_pos(g, n);
            let h = node_height(g, n);
            lo = lo.min(Vec2::new(pos[0], pos[1]));
            hi = hi.max(Vec2::new(pos[0] + node_width(g, n), pos[1] + h));
        }
        let size = (hi - lo).max(Vec2::splat(1.0));
        self.zoom = ((rect.width() - 40.0) / size.x).min((rect.height() - 40.0) / size.y).clamp(0.3, 1.0);
        let origin = canvas_origin(rect);
        let center = (lo + hi) * 0.5;
        self.pan = (rect.center() - origin) / self.zoom - center;
    }

    /// Places nodes that were never placed (at (0, 0)) to the left of the node that reads them,
    /// level with the socket they feed, then moves them down until they overlap nothing.
    fn layout(&self, g: &mut Graph) {
        let nodes = self.visible(g);
        // The Material Output anchors the whole-graph view at the origin.
        let anchor = match self.focus {
            Focus::Root => g.output_node(),
            Focus::Layer(_) => None,
        };
        let unplaced: Vec<NodeId> = nodes.iter().copied().filter(|n| Some(*n) != anchor && self.node_pos(g, *n) == [0.0, 0.0]).collect();
        if unplaced.is_empty() {
            return;
        }
        let layer = self.layer();
        // Depth = longest path to the sink (or the Material Output): readers are placed first.
        let mut depth: std::collections::BTreeMap<NodeId, usize> = Default::default();
        fn walk(g: &Graph, layer: Option<LayerRef>, n: NodeId, d: usize, depth: &mut std::collections::BTreeMap<NodeId, usize>) {
            if depth.get(&n).is_some_and(|x| *x >= d) || d > 64 {
                return;
            }
            depth.insert(n, d);
            for (_, c, _) in shown_links(g, layer, n) {
                walk(g, layer, c, d + 1, depth);
            }
        }
        match self.focus {
            Focus::Layer(_) => walk(g, layer, SINK, 0, &mut depth),
            Focus::Root => {
                if let Some(out) = g.output_node() {
                    walk(g, layer, out, 0, &mut depth);
                }
            }
        }
        let height = |g: &Graph, n: NodeId| node_height(g, n);
        let mut placed: Vec<NodeId> = nodes.iter().copied().filter(|n| !unplaced.contains(n)).collect();
        let mut order = unplaced;
        order.sort_by_key(|n| depth.get(n).copied().unwrap_or(1));
        for n in order {
            // The first placed node reading `n`, and the row of the socket it reads it on.
            let reader = placed.iter().copied().find_map(|r| {
                let (input, _, _) = shown_links(g, layer, r).into_iter().find(|(_, src, _)| *src == n)?;
                Some((r, input_row(g, r, &input).unwrap_or(0)))
            });
            let (x, mut y) = match reader {
                Some((r, row)) => {
                    let rp = self.node_pos(g, r);
                    (rp[0] - NODE_W - 50.0, rp[1] + row as f32 * ROW)
                }
                None => {
                    let d = depth.get(&n).copied().unwrap_or(1);
                    (-(d as f32) * (NODE_W + 50.0), 0.0)
                }
            };
            let h = height(g, n);
            // Move down past anything in the way.
            loop {
                let hit = placed.iter().copied().filter(|o| *o != SINK).find(|o| {
                    let op = self.node_pos(g, *o);
                    (op[0] - x).abs() < NODE_W + 20.0 && y < op[1] + height(g, *o) + 20.0 && op[1] < y + h + 20.0
                });
                match hit {
                    Some(o) => y = self.node_pos(g, o)[1] + height(g, o) + 20.0,
                    None => break,
                }
            }
            // Exactly [0, 0] means unplaced.
            set_pos(g, n, if [x, y] == [0.0, 0.0] { [0.0, 0.01] } else { [x, y] });
            placed.push(n);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        // The Selected layer view follows the Layers panel; a deleted or deselected layer falls
        // back to the whole graph instead of closing the editor.
        if let Focus::Layer(l) = self.focus {
            match p.selection.map(|s| s.layer()) {
                _ if ops::slot(&p.graph, l).is_none() => self.set_focus(Focus::Root),
                Some(sel) if sel != l => self.set_focus(Focus::Layer(sel)),
                None => self.set_focus(Focus::Root),
                _ => {}
            }
        }
        self.rect = ui.max_rect();
        // Every frame: nodes added elsewhere (new layers, effects) get placed as they appear.
        self.layout(&mut p.graph);
        if std::mem::take(&mut self.auto_layout_needed) {
            self.frame_needed = true;
        }
        self.header(ui, gpu, p);
        egui::Panel::right("node props").resizable(true).default_size(260.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.properties(ui, gpu, p));
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui, gpu, p));
    }

    /// "NODE EDITOR / <view>", the view switch and preview state, and the main inputs as keycaps.
    fn header(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        use crate::ui_theme as t;
        use crate::ui_widgets as w;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), t::H_SECTION), Sense::hover());
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t::DIVIDER));
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(t::PANEL_PAD, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut ui;
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(egui::RichText::new("NODE EDITOR").font(t::section()).color(t::TEXT_2).extra_letter_spacing(0.6));
        let selected = p.selection.map(|s| s.layer());
        let mut layer_view = self.layer().is_some();
        let views = [(false, "Whole graph"), (true, "Selected layer")];
        if w::segmented_enabled(ui, &mut layer_view, &views, 220.0, &[true, selected.is_some()]).changed() {
            match (layer_view, selected) {
                (true, Some(l)) => self.set_focus(Focus::Layer(l)),
                _ => self.set_focus(Focus::Root),
            }
        }
        if let Focus::Layer(l) = self.focus {
            let name = ops::slot(&p.graph, l).map(|s| s.name.clone()).unwrap_or_default();
            ui.label(egui::RichText::new(name).font(t::body_strong()).color(t::TEXT));
        }
        if let Some(l) = p.preview.clone() {
            let title = p.graph.get(l.node).map(|n| n.title().to_string()).unwrap_or_default();
            ui.colored_label(t::GOOD, format!("Previewing {title} › {}", pretty(&l.output)));
            if ui.add(w::secondary("Stop preview")).on_hover_text("Or Ctrl+Shift+click empty space").clicked() {
                p.set_preview(gpu, None);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = |ui: &mut egui::Ui, keys: &str, what: &str| -> egui::Response {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    w::keycap(ui, keys);
                    ui.label(egui::RichText::new(what).font(t::caption()).color(t::TEXT_3));
                })
                .response
            };
            // Right to left: the last hint first.
            hint(ui, "?", "All shortcuts").on_hover_ui(|ui| {
                egui::Grid::new("node editor shortcuts").num_columns(2).spacing([16.0, 3.0]).show(ui, |ui| {
                    for (keys, what) in SHORTCUTS {
                        ui.label(egui::RichText::new(*keys).font(t::value()).color(t::TEXT_2));
                        ui.label(*what);
                        ui.end_row();
                    }
                });
            });
            for (keys, what) in [("Ctrl+RMB", "Cut links"), ("Ctrl+Shift+Click", "Preview"), ("Tab", "Add")] {
                ui.add_space(6.0);
                hint(ui, keys, what);
            }
        });
    }

    fn to_screen(&self, origin: Pos2, pos: [f32; 2]) -> Pos2 {
        origin + (Vec2::new(pos[0], pos[1]) + self.pan) * self.zoom
    }

    fn to_canvas(&self, origin: Pos2, screen: Pos2) -> Pos2 {
        let c = (screen - origin) / self.zoom - self.pan;
        Pos2::new(c.x, c.y)
    }

    fn sink_pos(&self, g: &Graph) -> [f32; 2] {
        let content = self.layer().and_then(|l| ops::slot(g, l)).and_then(|s| s.content).and_then(|c| g.get(c));
        let x = content.map(|n| n.pos[0] + 240.0).unwrap_or(0.0).max(0.0);
        [x, content.map(|n| n.pos[1]).unwrap_or(0.0)]
    }

    fn node_pos(&self, g: &Graph, id: NodeId) -> [f32; 2] {
        if id == SINK {
            self.sink_pos(g)
        } else if is_virtual(id) {
            group_slot(g, id).map_or([0.0, 0.0], |s| s.node_pos)
        } else {
            g.node(id).pos
        }
    }

    fn node_rect(&self, g: &Graph, origin: Pos2, id: NodeId) -> Rect {
        Rect::from_min_size(self.to_screen(origin, self.node_pos(g, id)), Vec2::new(node_width(g, id), node_height(g, id)) * self.zoom)
    }

    /// Where links attach to an input. On a collapsed node every input attaches to the middle of
    /// the header's left edge.
    fn input_pos(&self, g: &Graph, origin: Pos2, id: NodeId, name: &str) -> Option<Pos2> {
        let r = self.node_rect(g, origin, id);
        let row = input_row(g, id, name)?;
        if is_collapsed(g, id) || is_reroute(g, id) {
            return Some(r.left_center());
        }
        Some(Pos2::new(r.left(), r.top() + (HEADER + ROW * (row as f32 + 0.5)) * self.zoom))
    }

    fn output_pos(&self, g: &Graph, origin: Pos2, id: NodeId, name: &str) -> Option<Pos2> {
        let r = self.node_rect(g, origin, id);
        let i = outputs(g, id).iter().position(|(n, _)| n == name)?;
        if is_collapsed(g, id) || is_reroute(g, id) {
            return Some(r.right_center());
        }
        Some(Pos2::new(r.right(), r.top() + (HEADER + ROW * (i as f32 + 0.5)) * self.zoom))
    }

    /// The collapse arrow's area in a node's header (screen).
    fn arrow_rect(&self, g: &Graph, origin: Pos2, id: NodeId) -> Rect {
        let r = self.node_rect(g, origin, id);
        Rect::from_min_size(r.min, Vec2::new(18.0, HEADER) * self.zoom)
    }

    /// The open button at the right of a layer group's header (screen).
    fn open_rect(&self, g: &Graph, origin: Pos2, id: NodeId) -> Rect {
        let r = self.node_rect(g, origin, id);
        Rect::from_min_size(Pos2::new(r.right() - HEADER * self.zoom, r.top()), Vec2::splat(HEADER * self.zoom))
    }

    /// Opens a layer group: selects its layer and shows it.
    fn open_group(&mut self, p: &mut Project, id: NodeId) {
        if let Some(l) = group_of(id).filter(|l| ops::slot(&p.graph, *l).is_some()) {
            p.selection = Some(ops::Selection::Layer(l));
            p.multi.clear();
            self.set_focus(Focus::Layer(l));
        }
    }

    /// The socket under a screen point: (socket, is output). Collapsed nodes have none.
    fn socket_at(&self, g: &Graph, origin: Pos2, nodes: &[NodeId], pt: Pos2) -> Option<(SocketRef, bool)> {
        for &n in nodes.iter().rev().filter(|n| !is_collapsed(g, **n)) {
            // A reroute's sockets are its ends; its middle moves it.
            let r = if is_reroute(g, n) { 9.0 * self.zoom } else { 10.0 * self.zoom.max(0.6) };
            for (name, _) in outputs(g, n) {
                if self.output_pos(g, origin, n, &name).is_some_and(|q| q.distance(pt) < r) {
                    return Some((SocketRef { node: n, name }, true));
                }
            }
            for (name, _, _) in inputs(g, n) {
                if self.input_pos(g, origin, n, &name).is_some_and(|q| q.distance(pt) < r) {
                    return Some((SocketRef { node: n, name }, false));
                }
            }
        }
        None
    }

    fn node_at(&self, g: &Graph, origin: Pos2, nodes: &[NodeId], pt: Pos2) -> Option<NodeId> {
        nodes.iter().rev().copied().find(|n| self.node_rect(g, origin, *n).contains(pt))
    }

    /// First input of `node` that accepts `ty`, preferring inputs without a value of their own,
    /// then an exact type match, then free inputs.
    fn input_for(g: &Graph, layer: Option<LayerRef>, node: NodeId, ty: SocketType) -> Option<String> {
        let linked: Vec<String> = links_into(g, layer, node).into_iter().map(|(i, _, _)| i).collect();
        let ins = inputs(g, node);
        let pick = |plain: bool, exact: bool, free: bool| {
            ins.iter()
                .find(|(n, _, t)| {
                    (!plain || input_value_def(g, node, n).is_none())
                        && (if exact { *t == ty } else { accepts(g, node, ty, *t) })
                        && (!free || !linked.contains(n))
                })
                .map(|(n, _, _)| n.clone())
        };
        [true, false].into_iter().find_map(|plain| {
            pick(plain, true, true).or_else(|| pick(plain, false, true)).or_else(|| pick(plain, true, false)).or_else(|| pick(plain, false, false))
        })
    }

    fn canvas(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        let rect = ui.available_rect_before_wrap();
        self.canvas_rect = rect;
        if !p.graph.frames.iter().map(|f| &f.nodes).eq(self.frames_seen.iter()) {
            self.selected.retain(|n| frame_of(*n).is_none());
            self.primary = self.primary.filter(|n| frame_of(*n).is_none());
        }
        self.canvas_inner(ui, gpu, p, rect);
        self.frames_seen = p.graph.frames.iter().map(|f| f.nodes.clone()).collect();
    }

    fn canvas_inner(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, rect: Rect) {
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, crate::ui_theme::BG_CANVAS);
        // Dot grid, 20 points apart at zoom 1.
        let step = 20.0 * self.zoom;
        if step >= 8.0 {
            let o = rect.min + Vec2::new((self.pan.x).rem_euclid(step), (self.pan.y).rem_euclid(step));
            let mut y = o.y;
            while y < rect.max.y {
                let mut x = o.x;
                while x < rect.max.x {
                    painter.circle_filled(Pos2::new(x, y), 1.0, crate::ui_theme::BORDER);
                    x += step;
                }
                y += step;
            }
        }
        if self.frame_needed || (resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Home))) {
            self.frame_all(&p.graph, rect);
            self.frame_needed = false;
        }
        let origin = canvas_origin(rect);
        let layer = self.layer();
        let (secondary_pressed, secondary_down, ctrl) =
            ui.input(|i| (i.pointer.secondary_pressed(), i.pointer.secondary_down(), i.modifiers.command));
        let menus_open = self.has_popup();
        self.param_rects.clear();
        let (pointer, mut pressed, primary_down, alt, shift, delta, scroll, middle) = ui.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.modifiers.alt,
                i.modifiers.shift,
                i.pointer.delta(),
                i.smooth_scroll_delta.y,
                i.pointer.middle_down(),
            )
        });

        // Zoom about the cursor.
        if resp.hovered() && scroll != 0.0 {
            let z = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.3, 2.5);
            if let Some(ptr) = pointer {
                let canvas = (ptr - origin) / self.zoom - self.pan;
                self.pan = (ptr - origin) / z - canvas;
            }
            self.zoom = z;
        }
        // Pan: right drag or middle drag (Alt + left drag is handled as a drag mode below).
        let rmb_tool = matches!(self.drag, Some(Drag::Cut { .. } | Drag::Lazy { .. } | Drag::Reroute { .. }));
        if (resp.dragged_by(egui::PointerButton::Secondary) && !rmb_tool) || (middle && resp.dragged()) {
            self.pan += delta / self.zoom;
        }

        let all = self.all_nodes(&p.graph);
        if all.is_empty() {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Empty graph: press Tab to add a node", egui::FontId::proportional(13.0), crate::ui_theme::TEXT_3);
        }

        // Frames, behind everything else.
        let frames = self.frame_rects(&p.graph, origin, &all);
        for (i, fr) in &frames {
            let selected = self.selected.contains(&frame_id(*i));
            painter.rect_filled(*fr, 8.0, Color32::from_white_alpha(9));
            let stroke = if selected { Stroke::new(1.5, crate::ui_theme::ACCENT) } else { Stroke::new(1.0, Color32::from_white_alpha(22)) };
            painter.rect_stroke(*fr, 8.0, stroke, egui::StrokeKind::Inside);
            let label = &p.graph.frames[*i].label;
            let at = fr.left_top() + Vec2::new(10.0, HEADER * 0.5) * self.zoom;
            let clip = Rect::from_min_size(fr.min, Vec2::new(fr.width(), HEADER * self.zoom)).intersect(rect);
            painter.with_clip_rect(clip).text(at, egui::Align2::LEFT_CENTER, label, egui::FontId::new(12.0 * self.zoom, egui::FontFamily::Name(crate::ui_theme::MEDIUM.into())), crate::ui_theme::TEXT_2);
        }

        // Links.
        for &n in &all {
            for (input, src, out) in shown_links(&p.graph, layer, n) {
                if !all.contains(&src) {
                    continue;
                }
                let (Some(a), Some(b)) = (self.output_pos(&p.graph, origin, src, &out), self.input_pos(&p.graph, origin, n, &input)) else {
                    continue;
                };
                let ty = outputs(&p.graph, src).into_iter().find(|(o, _)| *o == out).map_or(SocketType::Float, |(_, t)| t);
                draw_link(&painter, a, b, type_color(ty), self.zoom);
            }
        }

        // Nodes.
        let font = egui::FontId::proportional(12.0 * self.zoom);
        let link_ty = match &self.drag {
            Some(Drag::Link { ty, .. }) => Some(*ty),
            _ => None,
        };
        let rects: Vec<Rect> = all.iter().map(|n| self.node_rect(&p.graph, origin, *n)).collect();
        for (k, &n) in all.iter().enumerate() {
            let r = rects[k];
            let collapsed = is_collapsed(&p.graph, n);
            let selected = self.selected.contains(&n);
            if is_reroute(&p.graph, n) {
                let ty = outputs(&p.graph, n).first().map_or(SocketType::Float, |(_, t)| *t);
                painter.rect_filled(r, r.height() * 0.5, crate::ui_theme::BG_PANEL);
                let outline = if selected { Stroke::new(1.5, crate::ui_theme::ACCENT) } else { Stroke::new(1.0, crate::ui_theme::BORDER_STRONG) };
                painter.rect_stroke(r, r.height() * 0.5, outline, egui::StrokeKind::Inside);
                // Takes any type, so it's never dimmed while a link is dragged.
                let c = type_color(ty);
                painter.circle_filled(r.left_center(), 5.0 * self.zoom, c);
                painter.circle_filled(r.right_center(), 5.0 * self.zoom, c);
                continue;
            }
            let (title, cached) = if n == SINK {
                (format!("Layer: {}", layer.and_then(|l| ops::slot(&p.graph, l)).map(|s| s.name.as_str()).unwrap_or("")), false)
            } else if is_virtual(n) {
                (group_slot(&p.graph, n).map(|s| s.name.clone()).unwrap_or_default(), false)
            } else {
                let node = p.graph.node(n);
                (node.title().to_string(), node.cached)
            };
            painter.rect_filled(r, 6.0, crate::ui_theme::BG_PANEL);
            let header = Rect::from_min_size(r.min, Vec2::new(r.width(), HEADER * self.zoom));
            let round = if collapsed { egui::CornerRadius::same(6) } else { egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 } };
            painter.rect_filled(header, round, header_color(&p.graph, n));
            let mut title_x = 8.0;
            if is_virtual(n) && n != SINK {
                // Layer group: the open button.
                let o = self.open_rect(&p.graph, origin, n);
                let hot = pointer.is_some_and(|q| o.contains(q));
                if hot {
                    painter.rect_filled(o.shrink(3.0 * self.zoom), 4.0, Color32::from_white_alpha(28));
                }
                crate::icons::paint(&painter, crate::icons::Icon::Nodes, o.shrink(6.0 * self.zoom), if hot { crate::ui_theme::TEXT } else { crate::ui_theme::TEXT_2 });
            }
            if !is_virtual(n) {
                // Collapse arrow: pointing down when expanded, right when collapsed.
                let c = self.arrow_rect(&p.graph, origin, n).center();
                let s = 4.0 * self.zoom;
                let pts = if collapsed {
                    vec![c + Vec2::new(-s * 0.6, -s), c + Vec2::new(s * 0.8, 0.0), c + Vec2::new(-s * 0.6, s)]
                } else {
                    vec![c + Vec2::new(-s, -s * 0.6), c + Vec2::new(s, -s * 0.6), c + Vec2::new(0.0, s * 0.8)]
                };
                painter.add(egui::Shape::convex_polygon(pts, crate::ui_theme::TEXT_2, Stroke::NONE));
                title_x = 18.0;
            }
            let title_font = egui::FontId::new(font.size, egui::FontFamily::Name(crate::ui_theme::MEDIUM.into()));
            painter.text(header.left_center() + Vec2::new(title_x * self.zoom, 0.0), egui::Align2::LEFT_CENTER, &title, title_font, crate::ui_theme::TEXT);
            if cached {
                // A green "cached" tag.
                let tag_font = egui::FontId::monospace(9.5 * self.zoom);
                let g = painter.layout_no_wrap("cached".into(), tag_font, crate::ui_theme::GOOD);
                let tr = Rect::from_min_size(header.right_center() - Vec2::new(8.0 + g.size().x + 10.0 * self.zoom, g.size().y / 2.0 + 1.0), g.size() + Vec2::new(10.0 * self.zoom, 2.0));
                painter.rect_filled(tr, 3.0, Color32::from_rgba_unmultiplied(149, 223, 164, 36));
                painter.galley(tr.min + Vec2::new(5.0 * self.zoom, 1.0), g, crate::ui_theme::GOOD);
            }
            let previewed = p.preview.as_ref().is_some_and(|l| l.node == n);
            let outline = if selected {
                Stroke::new(1.5, crate::ui_theme::ACCENT)
            } else if previewed {
                Stroke::new(2.0, crate::ui_theme::SOCK_MATERIAL)
            } else {
                Stroke::new(1.0, crate::ui_theme::BORDER_STRONG)
            };
            if previewed {
                let out = p.preview.as_ref().map(|l| l.output.clone()).unwrap_or_default();
                if let Some(pos) = self.output_pos(&p.graph, origin, n, &out) {
                    painter.circle_stroke(pos, 8.0 * self.zoom, Stroke::new(2.0, crate::ui_theme::SOCK_MATERIAL));
                }
            }
            painter.rect_stroke(r, 6.0, outline, egui::StrokeKind::Inside);
            if collapsed {
                // Only connected sockets, on the header's edges.
                let linked_in = !links_into(&p.graph, layer, n).is_empty();
                let linked_out = all.iter().any(|m| shown_links(&p.graph, layer, *m).iter().any(|(_, src, _)| *src == n));
                if linked_in {
                    painter.circle_filled(r.left_center(), 5.0 * self.zoom, crate::ui_theme::SOCK_FLOAT);
                }
                if linked_out {
                    painter.circle_filled(r.right_center(), 5.0 * self.zoom, crate::ui_theme::SOCK_FLOAT);
                }
                continue;
            }
            for (name, ty) in outputs(&p.graph, n) {
                let pos = self.output_pos(&p.graph, origin, n, &name).unwrap();
                painter.circle_filled(pos, 5.0 * self.zoom, type_color(ty));
                painter.text(pos - Vec2::new(10.0 * self.zoom, 0.0), egui::Align2::RIGHT_CENTER, pretty(&name), font.clone(), crate::ui_theme::TEXT_2);
            }
            for (name, label, ty) in inputs(&p.graph, n) {
                let pos = self.input_pos(&p.graph, origin, n, &name).unwrap();
                // While dragging a link, dim inputs that can't accept it.
                let ok = link_ty.is_none_or(|lt| accepts(&p.graph, n, lt, ty));
                let c = if ok { type_color(ty) } else { type_color(ty).gamma_multiply(0.25) };
                painter.circle_filled(pos, 5.0 * self.zoom, c);
                // An unconnected input with a number value shows its name inside the field.
                let in_field = input_value(&p.graph, n, &name).is_some_and(|pd| param_span(pd) == 1);
                if !in_field {
                    let text = if name == NEW_LAYER || name == NEW_EFFECT { crate::ui_theme::TEXT_3 } else { crate::ui_theme::TEXT };
                    painter.text(pos + Vec2::new(10.0 * self.zoom, 0.0), egui::Align2::LEFT_CENTER, label, font.clone(), text);
                }
            }
            if !is_virtual(n) {
                // Drawn before the next node, so a node on top covers these.
                self.node_params(ui, gpu, p, n, r, rect, &rects[k + 1..]);
            }
        }

        if pressed && ctrl && shift && resp.hovered() && self.drag.is_none() && !menus_open {
            pressed = false;
            let hit = pointer.and_then(|ptr| self.node_at(&p.graph, origin, &all, ptr));
            if !hit.is_some_and(is_virtual) {
                self.cycle_preview(gpu, p, hit);
            }
        }
        if secondary_pressed && resp.hovered() && self.drag.is_none() && !menus_open {
            if let Some(ptr) = pointer {
                if alt {
                    if let Some(n) = self.node_at(&p.graph, origin, &all, ptr).filter(|n| !is_virtual(*n)) {
                        self.drag = Some(Drag::Lazy { from: n, menu: shift });
                    }
                } else if ctrl {
                    self.drag = Some(Drag::Cut { points: vec![ptr] });
                } else if shift {
                    self.drag = Some(Drag::Reroute { points: vec![ptr] });
                }
            }
        }

        // Presses on a parameter row select its node; the widget handles the rest.
        if pressed && self.drag.is_none() && !menus_open {
            let hit = pointer.and_then(|ptr| self.param_rects.iter().rev().find(|(_, r)| r.contains(ptr)).map(|(n, _)| *n));
            if let Some(n) = hit {
                pressed = false;
                if !self.selected.contains(&n) {
                    self.selected = BTreeSet::from([n]);
                }
                self.primary = Some(n);
            }
        }
        // The collapse arrow in a node's header, and a layer group's open button.
        if pressed && resp.hovered() && self.drag.is_none() && !menus_open {
            if let Some(ptr) = pointer {
                let top = self.node_at(&p.graph, origin, &all, ptr).filter(|n| *n != SINK);
                let arrow = |n: NodeId| !is_virtual(n) && !is_reroute(&p.graph, n) && self.arrow_rect(&p.graph, origin, n).contains(ptr);
                if let Some(n) = top.filter(|n| arrow(*n)) {
                    pressed = false;
                    p.checkpoint("Collapse nodes");
                    let node = p.graph.node_mut(n);
                    node.collapsed = !node.collapsed;
                } else if let Some(n) = top.filter(|n| is_virtual(*n) && self.open_rect(&p.graph, origin, *n).contains(ptr)) {
                    self.open_group(p, n);
                    return;
                }
            }
        }
        if resp.double_clicked() && !menus_open {
            if let Some(n) = pointer.and_then(|ptr| self.node_at(&p.graph, origin, &all, ptr)).filter(|n| group_of(*n).is_some()) {
                self.drag = None;
                self.open_group(p, n);
                return;
            }
        }

        // Start a drag with the left button (unless a menu is taking the click).
        if pressed && resp.hovered() && self.drag.is_none() && !menus_open {
            if let Some(ptr) = pointer {
                // Group outputs, and the links they feed, belong to the layer: they aren't dragged.
                let sock = self.socket_at(&p.graph, origin, &all, ptr).filter(|(s, is_output)| {
                    !is_virtual(s.node) || (s.node == SINK && !is_output)
                });
                let sock = sock.filter(|(s, is_output)| {
                    *is_output || !shown_links(&p.graph, layer, s.node).iter().any(|(i, src, _)| *i == s.name && is_virtual(*src))
                });
                if let Some((sock, is_output)) = sock {
                    if is_output {
                        let ty = p.graph.output_type(&graph::Link { node: sock.node, output: sock.name.clone() }).unwrap_or(SocketType::Float);
                        self.drag = Some(Drag::Link { from: sock, ty });
                    } else if let Some((_, src, out)) = links_into(&p.graph, layer, sock.node).into_iter().find(|(i, _, _)| *i == sock.name) {
                        // Pick up an existing link from its input end.
                        p.checkpoint("Disconnect");
                        disconnect(&mut p.graph, layer, &sock);
                        p.graph_changed(gpu);
                        // Keep the source visible while it's unconnected.
                        self.extra.insert(src);
                        let ty = p.graph.output_type(&graph::Link { node: src, output: out.clone() }).unwrap_or(SocketType::Float);
                        self.drag = Some(Drag::Link { from: SocketRef { node: src, name: out }, ty });
                    }
                } else if alt {
                    self.drag = Some(Drag::Pan);
                } else if let Some(n) = self.node_at(&p.graph, origin, &all, ptr) {
                    if shift {
                        if !self.selected.remove(&n) {
                            self.selected.insert(n);
                        }
                    } else if !self.selected.contains(&n) {
                        self.selected = BTreeSet::from([n]);
                    }
                    self.primary = Some(n);
                    self.drag = Some(Drag::Nodes);
                    self.moved = false;
                } else if let Some((i, _)) = frames.iter().rev().find(|(_, fr)| fr.contains(ptr)) {
                    let f = frame_id(*i);
                    if shift {
                        if !self.selected.remove(&f) {
                            self.selected.insert(f);
                        }
                    } else if !self.selected.contains(&f) {
                        self.selected = BTreeSet::from([f]);
                    }
                    self.primary = Some(f);
                    self.drag = Some(Drag::Nodes);
                    self.moved = false;
                } else {
                    if !shift {
                        self.selected.clear();
                        self.primary = None;
                    }
                    self.drag = Some(Drag::Box { start: ptr, additive: shift });
                }
            }
        }

        // Continue or finish the drag.
        if let Some(drag) = self.drag.clone() {
            match &drag {
                Drag::Nodes => {
                    if delta != Vec2::ZERO {
                        if !self.moved {
                            p.checkpoint("Move nodes");
                            self.moved = true;
                        }
                        let d = delta / self.zoom;
                        let mut moving: BTreeSet<NodeId> = self.selected.iter().copied().filter(|n| *n != SINK && frame_of(*n).is_none()).collect();
                        for i in self.selected.iter().filter_map(|n| frame_of(*n)) {
                            if let Some(f) = p.graph.frames.get(i) {
                                moving.extend(f.nodes.iter().copied().filter(|n| all.contains(n)));
                            }
                        }
                        for n in moving {
                            let q = self.node_pos(&p.graph, n);
                            set_pos(&mut p.graph, n, [q[0] + d.x, q[1] + d.y]);
                        }
                    }
                }
                Drag::Pan => self.pan += delta / self.zoom,
                Drag::Link { from, ty } => {
                    if let (Some(a), Some(b)) = (self.output_pos(&p.graph, origin, from.node, &from.name), pointer) {
                        draw_link(&painter, a, b, type_color(*ty), self.zoom);
                    }
                }
                Drag::Box { start, .. } => {
                    if let Some(ptr) = pointer {
                        let b = Rect::from_two_pos(*start, ptr);
                        painter.rect_filled(b, 0.0, crate::ui_theme::accent(0.14));
                        painter.rect_stroke(b, 0.0, Stroke::new(1.0, crate::ui_theme::accent(0.6)), egui::StrokeKind::Inside);
                    }
                }
                Drag::Cut { points } => {
                    if let (Some(ptr), Some(Drag::Cut { points })) = (pointer, &mut self.drag) {
                        if points.last().is_none_or(|l| l.distance(ptr) > 3.0) {
                            points.push(ptr);
                        }
                    }
                    painter.extend(egui::Shape::dashed_line(points, Stroke::new(1.5, crate::ui_theme::ERROR), 6.0, 4.0));
                }
                Drag::Reroute { points } => {
                    if let (Some(ptr), Some(Drag::Reroute { points })) = (pointer, &mut self.drag) {
                        if points.last().is_none_or(|l| l.distance(ptr) > 3.0) {
                            points.push(ptr);
                        }
                    }
                    painter.extend(egui::Shape::dashed_line(points, Stroke::new(1.5, crate::ui_theme::ACCENT), 6.0, 4.0));
                }
                Drag::Lazy { from, .. } => {
                    if let Some(ptr) = pointer {
                        let a = self.node_rect(&p.graph, origin, *from).right_center();
                        let target = self.node_at(&p.graph, origin, &all, ptr).filter(|b| b != from);
                        if let Some(b) = target {
                            let ok = !wrangler::link_options(&p.graph, layer, *from, b).is_empty();
                            let c = if ok { crate::ui_theme::SOCK_MATERIAL } else { crate::ui_theme::ERROR };
                            painter.rect_stroke(self.node_rect(&p.graph, origin, b), 5.0, Stroke::new(2.0, c), egui::StrokeKind::Outside);
                        }
                        painter.line_segment([a, ptr], Stroke::new(1.5, Color32::from_gray(200)));
                    }
                }
            }
            let released = match &drag {
                Drag::Cut { .. } | Drag::Lazy { .. } | Drag::Reroute { .. } => !secondary_down,
                _ => !primary_down,
            };
            if released {
                self.drag = None;
                match drag {
                    Drag::Link { from, ty } => self.drop_link(gpu, p, origin, &all, pointer, from, ty, rect),
                    Drag::Box { start, additive } => {
                        if let Some(ptr) = pointer {
                            let b = Rect::from_two_pos(start, ptr);
                            if !additive {
                                self.selected.clear();
                            }
                            for &n in &all {
                                if b.intersects(self.node_rect(&p.graph, origin, n)) {
                                    self.selected.insert(n);
                                    self.primary = Some(n);
                                }
                            }
                            for (i, fr) in &frames {
                                if b.contains_rect(*fr) {
                                    self.selected.insert(frame_id(*i));
                                }
                            }
                        }
                    }
                    Drag::Cut { points } => self.cut_links(gpu, p, origin, &all, &points),
                    Drag::Reroute { points } => self.add_reroutes(gpu, p, origin, &all, &points),
                    Drag::Lazy { from, menu } => {
                        if let Some(ptr) = pointer {
                            if let Some(b) = self.node_at(&p.graph, origin, &all, ptr).filter(|b| *b != from) {
                                if menu {
                                    self.lazy_menu = Some((ptr, from, b));
                                } else if !wrangler::link_options(&p.graph, layer, from, b).is_empty() {
                                    p.edit(gpu, "Lazy connect", |g| wrangler::lazy_connect(g, layer, from, b));
                                    self.extra.remove(&from);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Keys apply over the canvas, including its parameter rows, but not while typing.
        let over_params = pointer.is_some_and(|q| self.param_rects.iter().any(|(_, r)| r.contains(q)));
        let keys = (resp.hovered() || over_params) && !menus_open && !ui.ctx().text_edit_focused() && !egui::Popup::is_any_open(ui.ctx());
        if keys {
            if let Some(op) = ui.input(Self::shortcut_op) {
                self.apply(ui.ctx(), gpu, p, op);
            }
            let pasted = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Paste(text) => Some(text.clone()),
                    _ => None,
                })
            });
            if let Some(nodes) = pasted.as_deref().and_then(wrangler::parse_clipboard) {
                let at = self.to_canvas(origin, pointer.unwrap_or(rect.center()));
                let ids = p.edit(gpu, "Paste nodes", |g| wrangler::paste(g, &nodes, [at.x, at.y]));
                self.extra.extend(ids.iter().copied());
                self.primary = ids.first().copied();
                self.selected = ids.into_iter().collect();
            }
        }
        // Ctrl+Plus / Minus / 0 are merges here, not UI zoom.
        let over = resp.hovered();
        ui.ctx().options_mut(|o| o.zoom_with_keyboard = !over);

        // Keyboard: Tab adds a node, Delete removes the selected nodes.
        let tab = std::mem::take(&mut self.tab_pressed);
        let del = ui.input(|i| i.key_pressed(egui::Key::Delete));
        if tab && !menus_open {
            let at = self.to_canvas(origin, pointer.unwrap_or(rect.center()));
            self.add_menu = Some(AddMenu { at, search: String::new(), link: None });
        }
        if keys && del {
            let root = p.graph.root;
            let doomed: Vec<NodeId> = self
                .selected
                .iter()
                .copied()
                .filter(|n| !is_virtual(*n) && *n != root && p.graph.get(*n).is_some_and(|x| x.ty != "material_output"))
                .collect();
            let frames = self.take_selected_frames();
            if !doomed.is_empty() || !frames.is_empty() {
                p.edit(gpu, "Delete nodes", |g| {
                    remove_frames(g, &frames);
                    for n in &doomed {
                        g.remove(*n);
                    }
                });
                for n in doomed {
                    self.selected.remove(&n);
                    self.extra.remove(&n);
                }
                self.primary = None;
            }
        }
        if let Some(m) = crate::ui_widgets::dnd_release::<crate::meshmaps::MeshMap>(&resp) {
            let at = self.to_canvas(origin, pointer.unwrap_or(rect.center()));
            let id = p.edit(gpu, "Add mesh map", |g| {
                let id = g.add("mesh_map");
                g.set(id, "map", Value::Text(m.param_name().into()));
                g.node_mut(id).pos = [at.x, at.y - HEADER];
                id
            });
            self.extra.insert(id);
            self.selected = BTreeSet::from([id]);
            self.primary = Some(id);
        }
        if let Some(tex) = crate::ui_widgets::dnd_release::<crate::ui_library::TextureDrop>(&resp) {
            let at = self.to_canvas(origin, pointer.unwrap_or(rect.center()));
            // A file adds an Image node using it, a preset its Procedural node.
            let node: Option<(&str, Vec<(&str, String)>)> = match &*tex {
                crate::ui_library::TextureDrop::File(path) => match p.import_image(gpu, path) {
                    Ok(img) => Some(("image", vec![("image", img)])),
                    Err(e) => {
                        log::error!("{e:#}");
                        None
                    }
                },
                crate::ui_library::TextureDrop::Preset(k) => {
                    let preset = &crate::procedural::PRESETS[*k];
                    Some((preset.ty, preset.params.iter().map(|(key, v)| (*key, v.to_string())).collect()))
                }
            };
            if let Some((ty, params)) = node {
                let id = p.edit(gpu, "Add texture", |g| {
                    let id = g.add(ty);
                    for (key, v) in params {
                        g.set(id, key, Value::Text(v));
                    }
                    g.node_mut(id).pos = [at.x, at.y - HEADER];
                    id
                });
                self.extra.insert(id);
                self.selected = BTreeSet::from([id]);
                self.primary = Some(id);
            }
        }
        if resp.secondary_clicked() && !menus_open && !ctrl && !alt && !shift {
            let ptr = pointer.unwrap_or(rect.center());
            match self.node_at(&p.graph, origin, &all, ptr) {
                Some(n) if is_virtual(n) => {}
                // Right-click on a node: its Node Wrangler menu.
                Some(n) => {
                    if !self.selected.contains(&n) {
                        self.selected = BTreeSet::from([n]);
                    }
                    self.primary = Some(n);
                    self.node_menu = Some(ptr);
                }
                None => {
                    let at = self.to_canvas(origin, ptr);
                    self.add_menu = Some(AddMenu { at, search: String::new(), link: None });
                }
            }
        }
        self.add_menu_ui(ui, gpu, p, origin);
        self.node_menu_ui(ui, gpu, p);
        self.lazy_menu_ui(ui, gpu, p);
        self.param_popup_ui(ui, gpu, p);
    }

    /// A node's editable rows: its own parameters, and the values of unconnected inputs. Each is
    /// an editable widget, or text when zoomed out or covered by a node drawn on top (`above`).
    #[allow(clippy::too_many_arguments)]
    fn node_params(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, n: NodeId, r: Rect, canvas: Rect, above: &[Rect]) {
        // (first row, parameter, label) for every field shown.
        let mut fields: Vec<(usize, &'static graph::ParamDef, String)> = Vec::new();
        let mut row = outputs(&p.graph, n).len();
        for pd in own_params(&p.graph, n) {
            fields.push((row, pd, pretty(pd.name)));
            row += param_span(pd);
        }
        for (name, label, _) in inputs(&p.graph, n) {
            if let (Some(pd), Some(at)) = (input_value(&p.graph, n, &name), input_row(&p.graph, n, &name)) {
                fields.push((at, pd, label));
            }
        }
        let z = self.zoom;
        let row_rect = |k: usize| {
            let top = r.top() + (HEADER + ROW * k as f32) * z;
            Rect::from_min_size(Pos2::new(r.left() + 10.0 * z, top + (ROW - crate::ui_theme::H_CONTROL) * 0.5 * z), Vec2::new(NODE_W - 20.0, crate::ui_theme::H_CONTROL) * z)
        };
        for (first, pd, label) in fields {
            // A Vector's name on the first row, its components below.
            let parts: Vec<(Rect, Option<usize>)> = if pd.kind == graph::ParamKind::Vec3 {
                let name_row = row_rect(first);
                if canvas.intersects(name_row) {
                    ui.painter_at(canvas).text(name_row.left_center(), egui::Align2::LEFT_CENTER, &label, egui::FontId::proportional(12.0 * z), crate::ui_theme::TEXT_2);
                }
                (0..3).map(|k| (row_rect(first + 1 + k), Some(k))).collect()
            } else {
                vec![(row_rect(first), None)]
            };
            for (rr, component) in parts {
                if !canvas.intersects(rr) {
                    continue;
                }
                let v = p.graph.node(n).param(pd.name);
                if z < EDIT_ZOOM || above.iter().any(|o| o.intersects(rr)) {
                    let text = match component {
                        Some(k) => format!("{}: {}", ["X", "Y", "Z"][k], value_text(&Value::Float(v.as_vec3()[k]))),
                        None => format!("{label}: {}", value_text(&v)),
                    };
                    ui.painter_at(canvas).text(rr.left_center(), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(11.0 * z), crate::ui_theme::TEXT_3);
                    continue;
                }
                self.param_rects.push((n, rr));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rr)
                        .layout(egui::Layout::left_to_right(egui::Align::Center))
                        .id_salt(("node param", n, pd.name, component)),
                );
                child.set_clip_rect(rr.expand(1.0).intersect(canvas));
                let s = child.style_mut();
                for f in s.text_styles.values_mut() {
                    f.size *= z;
                }
                s.spacing.interact_size *= z;
                s.spacing.item_spacing *= z;
                s.spacing.button_padding *= z;
                s.spacing.icon_width *= z;
                s.spacing.icon_width_inner *= z;
                s.spacing.icon_spacing *= z;
                s.spacing.combo_width = rr.width();
                self.param_editor(&mut child, gpu, p, n, pd, &label, component, rr);
            }
        }
    }

    /// One parameter (or one component of a Vector), edited in place with the Properties
    /// panel's controls. See docs/reference/node-editor.md "Parameters".
    #[allow(clippy::too_many_arguments)]
    fn param_editor(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, n: NodeId, pd: &'static graph::ParamDef, label: &str, component: Option<usize>, row: Rect) {
        use crate::ui_layers::{param_value, track};
        use crate::ui_widgets::{Check, Field};
        let name = pd.name;
        let v = p.graph.node(n).param(name);
        let size = row.size();
        let full = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.allocate_ui_with_layout(size, egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| add(ui));
        };
        match pd.kind {
            graph::ParamKind::Vec3 => {
                let k = component.unwrap_or(0);
                let mut c = v.as_vec3();
                let r = ui.add(Field::new(&mut c[k], -1.0e6..=1.0e6).label(["X", "Y", "Z"][k]).decimals(3).speed(0.01).no_fill().width(size.x));
                track(p, gpu, &r, "Edit parameter", |g| g.set(n, name, Value::Vec3(c)));
            }
            graph::ParamKind::Float { min, max } => {
                let mut x = v.as_f32();
                let field = Field::new(&mut x, min..=max).label(label).width(size.x);
                let field = if max - min <= 2.0 { field } else { field.no_fill().speed(crate::ui_layers::drag_speed(min, max)) };
                let r = ui.add(field);
                track(p, gpu, &r, "Edit parameter", |g| g.set(n, name, Value::Float(x)));
            }
            graph::ParamKind::Int { min, max } => {
                let mut x = v.as_f32() as i32;
                let r = ui.add(Field::new(&mut x, min..=max).label(label).no_fill().speed(0.1).width(size.x));
                track(p, gpu, &r, "Edit parameter", |g| g.set(n, name, Value::Float(x as f32)));
            }
            graph::ParamKind::Bool => {
                let mut b = v.as_bool();
                let r = ui.add(Check::new(&mut b, label));
                track(p, gpu, &r, "Edit parameter", |g| g.set(n, name, Value::Bool(b)));
            }
            graph::ParamKind::Color => {
                let (text, _) = ui.allocate_exact_size(Vec2::new(size.x * 0.5, size.y), Sense::hover());
                ui.painter().text(text.left_center(), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(crate::ui_theme::FS_BODY * self.zoom), crate::ui_theme::TEXT_2);
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.spacing_mut().interact_size.x = size.x - text.width();
                param_value(ui, gpu, p, n, pd, name);
            }
            graph::ParamKind::Enum(_) | graph::ParamKind::Image => full(ui, &mut |ui| param_value(ui, gpu, p, n, pd, name)),
            graph::ParamKind::Stops => {
                let (r, resp) = ui.allocate_exact_size(size, Sense::click());
                if let Value::Stops(stops) = &v {
                    let strips = 32;
                    for i in 0..strips {
                        let (a, b) = (i as f32 / strips as f32, (i + 1) as f32 / strips as f32);
                        let c = ramp_color(stops, (a + b) * 0.5);
                        let x = |t: f32| r.left() + t * r.width();
                        let strip = Rect::from_x_y_ranges(x(a)..=x(b), r.y_range());
                        ui.painter().rect_filled(strip, 0.0, Color32::from(egui::Rgba::from_rgb(c[0], c[1], c[2])));
                    }
                }
                ui.painter().rect_stroke(r, crate::ui_theme::R_CONTROL, Stroke::new(1.0, crate::ui_theme::BORDER_STRONG), egui::StrokeKind::Inside);
                if resp.on_hover_text("Click to edit the stops").clicked() {
                    self.param_popup = Some((n, name, r.left_bottom()));
                }
            }
            graph::ParamKind::Colors => {
                let selected = if let Value::Colors(c) = &v { c.len() } else { 0 };
                let total = if p.maps.id_names.is_empty() { p.mesh.materials.len() } else { p.maps.id_names.len() };
                let button = egui::Button::new(format!("{selected} of {total} IDs")).fill(crate::ui_theme::BG_INPUT).min_size(size);
                if ui.add(button).clicked() {
                    self.param_popup = Some((n, name, row.left_bottom()));
                }
            }
        }
    }

    /// The ramp stop editor or ID list opened from a parameter row.
    fn param_popup_ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        let Some((n, name, at)) = self.param_popup else { return };
        if p.graph.get(n).is_none() {
            self.param_popup = None;
            return;
        }
        let area = egui::Area::new(egui::Id::new("node param popup")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(280.0);
                if name == "stops" { self.stops_editor(ui, gpu, p, n) } else { id_editor(ui, gpu, p, n) }
            });
        });
        // A click outside closes it, unless it lands in a color picker opened from it.
        let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pt| !area.response.rect.contains(pt)));
        let picker_open = egui::Popup::is_any_open(ui.ctx());
        if (outside && !picker_open) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.param_popup = None;
        }
    }

    fn shortcut_op(i: &egui::InputState) -> Option<Op> {
        use egui::{Key, Modifiers as M};
        let k = |m: M, key: Key| i.modifiers.matches_exact(m) && i.key_pressed(key);
        let ctrl = M::COMMAND;
        // Ctrl+X arrives as a Cut event rather than a key press.
        if i.events.iter().any(|e| matches!(e, egui::Event::Cut)) || k(ctrl, Key::X) {
            return Some(Op::Dissolve);
        }
        // Likewise Ctrl+C arrives as Copy (Ctrl+V as Paste, handled with its text).
        if i.events.iter().any(|e| matches!(e, egui::Event::Copy)) || k(ctrl, Key::C) {
            return Some(Op::Copy);
        }
        let ops = [
            (ctrl, Key::Num0, Op::Merge(MergeOp::Mix)),
            (ctrl, Key::Plus, Op::Merge(MergeOp::Add)),
            (ctrl, Key::Equals, Op::Merge(MergeOp::Add)),
            (ctrl, Key::Minus, Op::Merge(MergeOp::Subtract)),
            (ctrl, Key::Num8, Op::Merge(MergeOp::Multiply)),
            (ctrl, Key::Slash, Op::Merge(MergeOp::Divide)),
            (ctrl, Key::Comma, Op::Merge(MergeOp::Min)),
            (ctrl, Key::Period, Op::Merge(MergeOp::Max)),
            (M::ALT, Key::S, Op::Swap),
            (M::ALT, Key::X, Op::DeleteUnused),
            (M::SHIFT, Key::W, Op::AlignEdges(Edge::Top)),
            (M::SHIFT, Key::A, Op::AlignEdges(Edge::Left)),
            (M::SHIFT, Key::S, Op::AlignEdges(Edge::Bottom)),
            (M::SHIFT, Key::D, Op::AlignEdges(Edge::Right)),
            (M::SHIFT, Key::Equals, Op::Align),
            (M::NONE, Key::H, Op::ToggleCollapse),
            (M::NONE, Key::Backspace, Op::Reset),
            (M::SHIFT, Key::C, Op::CopySettings),
            (M::NONE, Key::K, Op::LinkToSelected),
            (M::NONE, Key::F, Op::Frame),
            (M::ALT, Key::P, Op::Unframe),
            (M::NONE, Key::A, Op::SelectAll),
            (M::ALT, Key::A, Op::DeselectAll),
        ];
        ops.into_iter().find(|(m, key, _)| k(*m, *key)).map(|(_, _, op)| op)
    }

    /// The selected nodes the operations apply to: not the sink or layer groups.
    fn selection(&self) -> Vec<NodeId> {
        self.selected.iter().copied().filter(|n| !is_virtual(*n)).collect()
    }

    fn apply(&mut self, ctx: &egui::Context, gpu: &Gpu, p: &mut Project, op: Op) {
        let layer = self.layer();
        let sel: Vec<NodeId> = self.selection().into_iter().filter(|n| p.graph.get(*n).is_some()).collect();
        match op {
            Op::Merge(m) => {
                if sel.len() < 2 {
                    return;
                }
                let mut g2 = p.graph.clone();
                if let Some(n) = wrangler::merge(&mut g2, layer, &sel, m) {
                    p.checkpoint("Merge nodes");
                    p.graph = g2;
                    p.graph_changed(gpu);
                    self.extra.insert(n);
                    self.selected = BTreeSet::from([n]);
                    self.primary = Some(n);
                }
            }
            Op::Swap => {
                let mut g2 = p.graph.clone();
                if wrangler::swap(&mut g2, layer, &sel) {
                    p.checkpoint("Swap links");
                    p.graph = g2;
                    p.graph_changed(gpu);
                }
            }
            Op::Dissolve => {
                let root = p.graph.root;
                let doomed: Vec<NodeId> =
                    sel.into_iter().filter(|n| *n != root && p.graph.node(*n).ty != "material_output").collect();
                let frames = self.take_selected_frames();
                if !doomed.is_empty() || !frames.is_empty() {
                    p.edit(gpu, "Dissolve nodes", |g| {
                        remove_frames(g, &frames);
                        for n in &doomed {
                            wrangler::dissolve(g, layer, *n);
                        }
                    });
                    self.selected.clear();
                    self.primary = None;
                }
            }
            Op::DeleteUnused => {
                let live: BTreeSet<NodeId> = p.graph.output_node().map(|o| p.graph.subgraph(o)).unwrap_or_default();
                if p.graph.nodes.keys().any(|k| !live.contains(k)) {
                    p.edit(gpu, "Delete unused nodes", |g| {
                        g.collect_garbage();
                    });
                    self.extra.clear();
                    self.selected.retain(|n| is_virtual(*n) || p.graph.get(*n).is_some());
                    self.primary = self.primary.filter(|n| p.graph.get(*n).is_some());
                }
            }
            Op::Duplicate => {
                if sel.is_empty() {
                    return;
                }
                let copies = p.edit(gpu, "Duplicate nodes", |g| wrangler::duplicate(g, &sel));
                self.extra.extend(copies.iter().copied());
                self.primary = copies.first().copied();
                self.selected = copies.into_iter().collect();
            }
            Op::Align => {
                if sel.len() > 1 {
                    p.checkpoint("Align nodes");
                    wrangler::align(&mut p.graph, &sel);
                }
            }
            Op::AlignEdges(edge) => {
                if sel.len() > 1 {
                    p.checkpoint("Align nodes");
                    wrangler::align_edges(&mut p.graph, &sel, edge);
                }
            }
            Op::ToggleCollapse => {
                let sel: Vec<NodeId> = sel.into_iter().filter(|n| !is_reroute(&p.graph, *n)).collect();
                if !sel.is_empty() {
                    let all_collapsed = sel.iter().all(|n| p.graph.node(*n).collapsed);
                    p.checkpoint(if all_collapsed { "Expand nodes" } else { "Collapse nodes" });
                    for n in &sel {
                        p.graph.node_mut(*n).collapsed = !all_collapsed;
                    }
                }
            }
            Op::Copy => {
                if let Some(text) = wrangler::copy_nodes(&p.graph, &sel) {
                    ctx.copy_text(text);
                }
            }
            Op::Reset => {
                if !sel.is_empty() {
                    p.edit(gpu, "Reset nodes", |g| {
                        for n in &sel {
                            wrangler::reset(g, *n);
                        }
                    });
                }
            }
            Op::CopySettings => {
                if let Some(from) = self.primary.filter(|n| sel.contains(n)) {
                    let mut g2 = p.graph.clone();
                    if wrangler::copy_settings(&mut g2, from, &sel) > 0 {
                        p.checkpoint("Copy node settings");
                        p.graph = g2;
                        p.graph_changed(gpu);
                    }
                }
            }
            Op::LinkToSelected => {
                if let Some(active) = self.primary.filter(|n| sel.contains(n)) {
                    let mut g2 = p.graph.clone();
                    if wrangler::link_to_selected(&mut g2, layer, active, &sel) > 0 {
                        p.checkpoint("Link to selected");
                        p.graph = g2;
                        p.graph_changed(gpu);
                        self.extra.remove(&active);
                    }
                }
            }
            Op::SelectAll => {
                self.selected = self.all_nodes(&p.graph).into_iter().collect();
                let shown: BTreeSet<NodeId> = self.selected.clone();
                for (i, f) in p.graph.frames.iter().enumerate() {
                    if f.nodes.iter().any(|n| shown.contains(n)) {
                        self.selected.insert(frame_id(i));
                    }
                }
            }
            Op::Frame => {
                let nodes: BTreeSet<NodeId> = sel.iter().copied().filter(|n| p.graph.get(*n).is_some()).collect();
                if !nodes.is_empty() {
                    let f = p.edit(gpu, "Frame nodes", |g| {
                        for f in &mut g.frames {
                            f.nodes.retain(|n| !nodes.contains(n));
                        }
                        g.prune_frames();
                        g.frames.push(graph::Frame { label: "Frame".into(), nodes });
                        frame_id(g.frames.len() - 1)
                    });
                    self.selected = BTreeSet::from([f]);
                    self.primary = Some(f);
                }
            }
            Op::Unframe => {
                if p.graph.frames.iter().any(|f| sel.iter().any(|n| f.nodes.contains(n))) {
                    p.edit(gpu, "Remove from frame", |g| {
                        for f in &mut g.frames {
                            f.nodes.retain(|n| !sel.contains(n));
                        }
                        g.prune_frames();
                    });
                    // Frame indices changed.
                    self.selected.retain(|n| frame_of(*n).is_none());
                    self.primary = self.primary.filter(|n| frame_of(*n).is_none());
                }
            }
            Op::DeselectAll => {
                self.selected.clear();
                self.primary = None;
            }
        }
    }

    /// Ctrl+Shift+click: shows the node's first output in the viewport; clicking the same node
    /// again steps through its outputs, then turns the preview off. Empty space turns it off.
    fn cycle_preview(&mut self, gpu: &Gpu, p: &mut Project, node: Option<NodeId>) {
        let next = node.filter(|n| !is_virtual(*n)).and_then(|n| {
            let outs = outputs(&p.graph, n);
            let i = match &p.preview {
                Some(l) if l.node == n => outs.iter().position(|(o, _)| *o == l.output).map_or(0, |i| i + 1),
                _ => 0,
            };
            outs.get(i).map(|(o, _)| graph::Link { node: n, output: o.clone() })
        });
        p.set_preview(gpu, next);
    }

    /// The frames shown (those with a node shown) and their screen rectangles: around their
    /// shown nodes, with FRAME_PAD around and a header for the label.
    fn frame_rects(&self, g: &Graph, origin: Pos2, shown: &[NodeId]) -> Vec<(usize, Rect)> {
        let mut out = Vec::new();
        for (i, f) in g.frames.iter().enumerate() {
            let r = f.nodes.iter().filter(|n| shown.contains(n)).map(|n| self.node_rect(g, origin, *n)).reduce(|a, b| a.union(b));
            if let Some(r) = r {
                let pad = FRAME_PAD * self.zoom;
                out.push((i, Rect::from_min_max(r.min - Vec2::new(pad, pad + HEADER * self.zoom), r.max + Vec2::splat(pad))));
            }
        }
        out
    }

    /// Takes the selected frames out of the selection, for deleting them with `remove_frames`.
    fn take_selected_frames(&mut self) -> BTreeSet<usize> {
        let frames: BTreeSet<usize> = self.selected.iter().filter_map(|n| frame_of(*n)).collect();
        self.selected.retain(|n| frame_of(*n).is_none());
        self.primary = self.primary.filter(|n| frame_of(*n).is_none());
        frames
    }

    /// Puts reroutes on the links crossed by a Shift+right-drag stroke: one per output, at the
    /// average of its links' crossings. Only links into a node's own inputs are rerouted.
    fn add_reroutes(&mut self, gpu: &Gpu, p: &mut Project, origin: Pos2, all: &[NodeId], stroke: &[Pos2]) {
        let layer = self.layer();
        // (source, output) -> (crossings, targets)
        type Crossings = (Vec<Pos2>, Vec<(NodeId, String)>);
        let mut found: std::collections::BTreeMap<(NodeId, String), Crossings> = Default::default();
        for &n in all {
            if is_virtual(n) {
                continue;
            }
            for (input, src, out) in links_into(&p.graph, layer, n) {
                // A node's own inputs only: not Stack layers or Mask Stack effects.
                if !p.graph.node(n).inputs.contains_key(&input) || !all.contains(&src) {
                    continue;
                }
                let (Some(a), Some(b)) = (self.output_pos(&p.graph, origin, src, &out), self.input_pos(&p.graph, origin, n, &input)) else {
                    continue;
                };
                let curve = link_points(a, b, self.zoom);
                let hit = curve.windows(2).find_map(|c| stroke.windows(2).find_map(|s| segment_intersection(c[0], c[1], s[0], s[1])));
                if let Some(at) = hit {
                    let e = found.entry((src, out)).or_default();
                    e.0.push(at);
                    e.1.push((n, input));
                }
            }
        }
        if found.is_empty() {
            return;
        }
        let places: Vec<_> = found
            .into_iter()
            .map(|(from, (hits, targets))| {
                let mean = hits.iter().fold(Vec2::ZERO, |s, q| s + q.to_vec2()) / hits.len() as f32;
                let c = self.to_canvas(origin, mean.to_pos2());
                (from, [c.x - REROUTE.x * 0.5, c.y - REROUTE.y * 0.5], targets)
            })
            .collect();
        p.edit(gpu, "Add reroutes", |g| {
            for ((src, out), pos, targets) in places {
                let r = g.add("reroute");
                g.node_mut(r).pos = pos;
                g.connect(src, &out, r, "in");
                for (n, input) in targets {
                    g.connect(r, "out", n, &input);
                }
            }
        });
    }

    /// Removes the links crossed by a Ctrl+right-drag stroke.
    fn cut_links(&mut self, gpu: &Gpu, p: &mut Project, origin: Pos2, all: &[NodeId], stroke: &[Pos2]) {
        let layer = self.layer();
        let mut cut = Vec::new();
        for &n in all {
            for (input, src, out) in shown_links(&p.graph, layer, n) {
                if !all.contains(&src) || is_virtual(src) {
                    continue;
                }
                let (Some(a), Some(b)) = (self.output_pos(&p.graph, origin, src, &out), self.input_pos(&p.graph, origin, n, &input)) else {
                    continue;
                };
                let curve = link_points(a, b, self.zoom);
                let hit = curve.windows(2).any(|c| stroke.windows(2).any(|s| segments_cross(c[0], c[1], s[0], s[1])));
                if hit {
                    cut.push(SocketRef { node: n, name: input });
                    // Keep the source visible while it's unconnected.
                    self.extra.insert(src);
                }
            }
        }
        if !cut.is_empty() {
            p.edit(gpu, "Cut links", |g| {
                // Highest index first, so removing slot links keeps the other indices valid.
                for s in cut.iter().rev() {
                    disconnect(g, layer, s);
                }
            });
        }
    }

    fn node_menu_ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        let Some(at) = self.node_menu else { return };
        let mut chosen = None;
        let mut preview = false;
        let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let n_sel = self.selection().len();
        let area = egui::Area::new(egui::Id::new("node wrangler menu")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_width(240.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    let mut item = |ui: &mut egui::Ui, label: &str, key: &str, enabled: bool, op: Option<Op>| {
                        let r = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(key).frame(false));
                        if r.clicked() {
                            match op {
                                Some(op) => chosen = Some(op),
                                None => preview = true,
                            }
                        }
                    };
                    item(ui, "Preview output", "Ctrl+Shift+Click", true, None);
                    item(ui, "Collapse / expand", "H", true, Some(Op::ToggleCollapse));
                    item(ui, "Copy", "Ctrl+C", true, Some(Op::Copy));
                    ui.separator();
                    ui.label(egui::RichText::new("Merge selected").small().weak());
                    for m in MergeOp::ALL {
                        item(ui, m.label(), m.shortcut(), n_sel > 1, Some(Op::Merge(m)));
                    }
                    ui.separator();
                    item(ui, "Swap links", "Alt+S", n_sel == 1 || n_sel == 2, Some(Op::Swap));
                    item(ui, "Link active to selected", "K", n_sel > 1, Some(Op::LinkToSelected));
                    item(ui, "Copy settings to selected", "Shift+C", n_sel > 1, Some(Op::CopySettings));
                    item(ui, "Reset to defaults", "Backspace", true, Some(Op::Reset));
                    ui.separator();
                    item(ui, "Duplicate", "", true, Some(Op::Duplicate));
                    item(ui, "Align in a row or column", "Shift+=", n_sel > 1, Some(Op::Align));
                    item(ui, "Align top edges", "Shift+W", n_sel > 1, Some(Op::AlignEdges(Edge::Top)));
                    item(ui, "Align left edges", "Shift+A", n_sel > 1, Some(Op::AlignEdges(Edge::Left)));
                    item(ui, "Align bottom edges", "Shift+S", n_sel > 1, Some(Op::AlignEdges(Edge::Bottom)));
                    item(ui, "Align right edges", "Shift+D", n_sel > 1, Some(Op::AlignEdges(Edge::Right)));
                    item(ui, "Dissolve (delete and reconnect)", "Ctrl+X", true, Some(Op::Dissolve));
                    item(ui, "Delete unused nodes", "Alt+X", true, Some(Op::DeleteUnused));
                });
            });
        });
        let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pt| !area.response.rect.contains(pt)));
        if outside || chosen.is_some() || preview {
            close = true;
        }
        if preview {
            let n = self.primary;
            // Start from the first output, even if this node is already previewed.
            if p.preview.as_ref().is_some_and(|l| Some(l.node) == n) {
                p.set_preview(gpu, None);
            }
            self.cycle_preview(gpu, p, n);
        }
        if let Some(op) = chosen {
            self.apply(ui.ctx(), gpu, p, op);
        }
        if close {
            self.node_menu = None;
        }
    }

    /// Shift+Alt+right-drag: choose which output goes to which input.
    fn lazy_menu_ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        let Some((at, a, b)) = self.lazy_menu else { return };
        let layer = self.layer();
        if p.graph.get(a).is_none() || (b != SINK && p.graph.get(b).is_none()) || (is_virtual(b) && b != SINK) {
            self.lazy_menu = None;
            return;
        }
        let opts = wrangler::link_options(&p.graph, layer, a, b);
        let mut chosen = None;
        let area = egui::Area::new(egui::Id::new("lazy connect menu")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(200.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    if opts.is_empty() {
                        ui.label(egui::RichText::new("No compatible sockets").weak());
                    }
                    let in_label = |name: &str| inputs(&p.graph, b).into_iter().find(|(n, _, _)| n == name).map(|(_, l, _)| l).unwrap_or_default();
                    for (out, input) in &opts {
                        if ui.add(egui::Button::new(format!("{}  →  {}", pretty(out), in_label(input))).frame(false)).clicked() {
                            chosen = Some((out.clone(), input.clone()));
                        }
                    }
                });
            });
        });
        let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pt| !area.response.rect.contains(pt)));
        if let Some((out, input)) = chosen.clone() {
            let mut g2 = p.graph.clone();
            if connect(&mut g2, layer, &SocketRef { node: a, name: out }, &SocketRef { node: b, name: input }) {
                p.checkpoint("Connect");
                p.graph = g2;
                p.graph_changed(gpu);
                self.extra.remove(&a);
            }
        }
        if chosen.is_some() || outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.lazy_menu = None;
        }
    }

    /// Releases a dragged link: onto an input connects; onto a node connects to its first
    /// fitting input; onto empty space opens the add menu to create a connected node.
    #[allow(clippy::too_many_arguments)]
    fn drop_link(&mut self, gpu: &Gpu, p: &mut Project, origin: Pos2, all: &[NodeId], pointer: Option<Pos2>, from: SocketRef, ty: SocketType, rect: Rect) {
        let Some(pt) = pointer else { return };
        let layer = self.layer();
        let target = match self.socket_at(&p.graph, origin, all, pt) {
            Some((s, false)) => Some(s),
            Some((_, true)) => None,
            None => self
                .node_at(&p.graph, origin, all, pt)
                .filter(|n| *n != from.node)
                .and_then(|n| Self::input_for(&p.graph, layer, n, ty).map(|name| SocketRef { node: n, name })),
        };
        match target {
            Some(to) => {
                let mut g2 = p.graph.clone();
                if connect(&mut g2, layer, &from, &to) {
                    p.checkpoint("Connect");
                    p.graph = g2;
                    self.extra.remove(&from.node);
                    p.graph_changed(gpu);
                }
            }
            None if rect.contains(pt) && self.node_at(&p.graph, origin, all, pt).is_none() => {
                let at = self.to_canvas(origin, pt);
                self.add_menu = Some(AddMenu { at, search: String::new(), link: Some((from, ty)) });
            }
            None => {}
        }
    }

    fn add_menu_ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, origin: Pos2) {
        let Some(mut menu) = self.add_menu.take() else { return };
        let screen = self.to_screen(origin, [menu.at.x, menu.at.y]);
        let mut keep = true;
        let mut chosen = None;
        let link_ty = menu.link.as_ref().map(|(_, t)| *t);
        let area = egui::Area::new(egui::Id::new("add node")).fixed_pos(screen).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(260.0);
                if link_ty.is_some() {
                    ui.label(egui::RichText::new("Add a connected node").small().weak());
                }
                let r = ui.add(egui::TextEdit::singleline(&mut menu.search).hint_text("Search nodes…").desired_width(f32::INFINITY));
                r.request_focus();
                let q = menu.search.to_ascii_lowercase();
                let mut first = None;
                egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        let mut category = None;
                        for e in add_entries() {
                            let d = graph::def(e.ty).expect("node type");
                            if d.category == graph::Category::Output {
                                continue;
                            }
                            if !q.is_empty() && !e.title.to_ascii_lowercase().contains(&q) {
                                continue;
                            }
                            // With a link, only offer nodes that can take it.
                            if let Some(lt) = link_ty {
                                let accepts = d.input_sockets().iter().any(|(_, t)| compatible(lt, *t))
                                    || d.ty == "reroute"
                                    || (d.ty == "stack" && lt == SocketType::Material)
                                    || (d.ty == "mask_stack" && compatible(lt, SocketType::Float));
                                if !accepts {
                                    continue;
                                }
                            }
                            if category != Some(d.category) {
                                category = Some(d.category);
                                category_label(ui, d.category);
                            }
                            first.get_or_insert(e);
                            if ui.add(egui::Button::new(e.title).frame(false)).clicked() {
                                chosen = Some(e);
                            }
                        }
                    });
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    chosen = first;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
        // A click outside the menu closes it.
        let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pt| !area.response.rect.contains(pt)));
        if outside {
            keep = false;
        }
        if let Some(entry) = chosen {
            let layer = self.layer();
            let at = menu.at;
            let link = menu.link.clone();
            let id = p.edit(gpu, "Add node", |g| {
                let id = g.add(entry.ty);
                if let Some((k, v)) = entry.set {
                    g.set(id, k, Value::Text(v.into()));
                }
                g.node_mut(id).pos = [at.x, at.y];
                if let Some((from, lt)) = &link {
                    if let Some(name) = Self::input_for(g, layer, id, *lt) {
                        connect(g, layer, from, &SocketRef { node: id, name });
                    }
                }
                id
            });
            if let Some((from, _)) = &menu.link {
                self.extra.remove(&from.node);
            }
            self.extra.insert(id);
            self.selected = BTreeSet::from([id]);
            self.primary = Some(id);
            keep = false;
        }
        if keep {
            self.add_menu = Some(menu);
        }
    }

    /// The sidebar, laid out like the Properties panel. See docs/reference/node-editor.md
    /// "Properties sidebar".
    fn properties(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
        use crate::ui_theme as t;
        use crate::ui_widgets as w;
        ui.add_space(8.0);
        if let Some(i) = self.primary.and_then(frame_of).filter(|i| *i < p.graph.frames.len()) {
            ui.label(egui::RichText::new("Frame").font(t::body_strong()).color(t::TEXT));
            ui.add_space(4.0);
            w::setting(ui, "Label", |ui| {
                let old = p.graph.frames[i].label.clone();
                let mut label = old.clone();
                let edit = egui::TextEdit::singleline(&mut label).desired_width(f32::INFINITY).min_size(Vec2::new(0.0, t::H_CONTROL)).vertical_align(egui::Align::Center);
                if ui.add(edit).lost_focus() && label != old {
                    p.edit(gpu, "Rename frame", |g| g.frames[i].label = label);
                }
            });
            return;
        }
        let count = self.selection().len();
        if count > 1 {
            w::hint(ui, format!("{count} nodes selected"));
        }
        let Some(n) = self.primary.filter(|n| !is_virtual(*n) && p.graph.get(*n).is_some()) else {
            w::hint(ui, "Select a node.");
            return;
        };
        let node = p.graph.node(n).clone();
        let def = node.def();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, header_color(&p.graph, n));
            ui.label(egui::RichText::new(node.title()).font(t::body_strong()).color(t::TEXT));
            w::hint(ui, def.category.name());
        });
        ui.add_space(4.0);
        w::setting(ui, "Label", |ui| {
            let mut label = node.label.clone();
            let edit = egui::TextEdit::singleline(&mut label).hint_text(def.title).desired_width(f32::INFINITY).min_size(Vec2::new(0.0, t::H_CONTROL)).vertical_align(egui::Align::Center);
            if ui.add(edit).lost_focus() && label != node.label {
                p.edit(gpu, "Rename node", |g| g.node_mut(n).label = label);
            }
        });
        w::setting(ui, "Cached", |ui| {
            let mut cached = node.cached;
            let r = ui.add_enabled(!def.non_local && crate::compile::cacheable(&node), w::Toggle(&mut cached));
            if r.changed() {
                p.edit(gpu, "Cached", |g| g.node_mut(n).cached = cached);
            }
        });
        if def.non_local {
            w::hint(ui, "Filters read neighboring texels and are always cached.");
        }
        if crate::graph::GENERATORS.contains(&node.ty.as_str()) && ui.add(w::secondary("Ungroup")).on_hover_text("Replace with its component nodes").clicked() {
            if let Some(out) = p.edit(gpu, "Ungroup", |g| ungroup(g, n)) {
                self.selected = BTreeSet::from([out]);
                self.primary = Some(out);
                self.auto_layout_needed = true;
            }
            return;
        }
        if !def.params.is_empty() {
            w::Section::new("Parameters").id_salt("node params").show(ui, |ui| {
                w::hint(ui, "Edit them on the node; the pin exposes one on the layer.");
                for pd in def.params.iter().filter(|pd| node.shows(pd)) {
                    ui.horizontal(|ui| {
                        crate::ui_layers::expose_pin(ui, gpu, p, n, pd.name);
                        ui.label(pretty(pd.name));
                    });
                }
            });
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
                changed |= crate::ui_widgets::color_edit(ui, c).changed();
                if ui.add(crate::ui_widgets::ghost_icon(crate::icons::Icon::Close)).clicked() {
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

}

/// ID Select's color list: a checkbox per ID in the ID map (the high poly's materials when
/// the ID map was baked from it, otherwise the mesh's).
pub fn id_editor(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, n: NodeId) {
    let Value::Colors(colors) = p.graph.node(n).param("colors") else { return };
    let names = if p.maps.id_names.is_empty() { p.mesh.materials.clone() } else { p.maps.id_names.clone() };
    ui.label(if p.maps.id_names.is_empty() { "Selected IDs (mesh materials)" } else { "Selected IDs (high-poly materials)" });
    if p.maps.status(crate::meshmaps::MeshMap::Id) == crate::meshmaps::MapStatus::Missing {
        ui.label(egui::RichText::new("Bake the ID map (Bake Mesh Maps, above the viewport) for this to have an effect.").weak().small());
    }
    let mut new = colors.clone();
    let same = |x: &[f32; 3], c: &[f32; 3]| (x[0] - c[0]).abs() + (x[1] - c[1]).abs() + (x[2] - c[2]).abs() < 1e-3;
    for (i, name) in names.iter().enumerate() {
        let c = id_color(i as u32);
        let mut on = colors.iter().any(|x| same(x, &c));
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
            if ui.checkbox(&mut on, name).changed() {
                new.retain(|x| !same(x, &c));
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

/// A parameter's value as short text (zoomed-out or covered rows).
fn value_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "on" } else { "off" }.into(),
        Value::Float(f) => format!("{f:.3}").trim_end_matches('0').trim_end_matches('.').to_string(),
        Value::Vec3(c) => format!("{:.2}, {:.2}, {:.2}", c[0], c[1], c[2]),
        Value::Text(t) if t.is_empty() => "(none)".into(),
        Value::Text(t) => t.clone(),
        Value::Stops(s) => format!("{} stops", s.len()),
        Value::Colors(c) => format!("{} IDs", c.len()),
    }
}

/// An add menu item: a node type, optionally with a choice set (Factor, Color).
#[derive(Clone, Copy)]
struct AddEntry {
    title: &'static str,
    ty: &'static str,
    set: Option<(&'static str, &'static str)>,
}

/// The add menu's items in category order: every node type, plus Factor and Color after the
/// constants. See docs/reference/node-editor.md "Add menu".
fn add_entries() -> Vec<AddEntry> {
    let mut v = Vec::new();
    for c in graph::Category::ALL {
        for d in graph::defs().iter().filter(|d| d.category == c) {
            v.push(AddEntry { title: d.title, ty: d.ty, set: None });
            match d.ty {
                "value" => v.push(AddEntry { title: "Factor", ty: "value", set: Some(("type", "Factor")) }),
                "vector" => v.push(AddEntry { title: "Color", ty: "vector", set: Some(("type", "Color")) }),
                _ => {}
            }
        }
    }
    v
}

/// An add menu category: a square of its header color and its name in small capitals.
fn category_label(ui: &mut egui::Ui, c: graph::Category) {
    use crate::ui_theme as t;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (r, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
        let [red, green, blue] = c.color();
        ui.painter().rect_filled(r, 2.0, Color32::from_rgb(red, green, blue));
        ui.label(egui::RichText::new(c.name().to_uppercase()).font(t::section()).color(t::TEXT_3).extra_letter_spacing(0.6));
    });
}

/// The ramp's linear color at `t`, interpolating between stops sorted by position.
fn ramp_color(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let Some(first) = stops.first() else { return [0.0; 3] };
    if t <= first.0 {
        return first.1;
    }
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let f = if b > a { (t - a) / (b - a) } else { 1.0 };
            return [0, 1, 2].map(|k| ca[k] + (cb[k] - ca[k]) * f);
        }
    }
    stops.last().map(|s| s.1).unwrap_or([0.0; 3])
}

/// Screen position of canvas (0, 0) before panning. Anchored at the top-left corner, so the
/// canvas changing width (the properties sidebar resizing) doesn't move the nodes.
fn canvas_origin(rect: Rect) -> Pos2 {
    rect.left_top() + Vec2::new(NODE_W * 2.0, 160.0)
}

/// Points along a link's curve (the same curve `draw_link` draws).
fn link_points(a: Pos2, b: Pos2, zoom: f32) -> Vec<Pos2> {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    let (c1, c2) = (a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0));
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let v = a.to_vec2() * (u * u * u) + c1.to_vec2() * (3.0 * u * u * t) + c2.to_vec2() * (3.0 * u * t * t) + b.to_vec2() * (t * t * t);
            v.to_pos2()
        })
        .collect()
}

/// Where segments p1-p2 and q1-q2 cross, if they do.
fn segment_intersection(p1: Pos2, p2: Pos2, q1: Pos2, q2: Pos2) -> Option<Pos2> {
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    let (r, s) = (p2 - p1, q2 - q1);
    let d = cross(r, s);
    if d.abs() < 1e-6 {
        return None;
    }
    let t = cross(q1 - p1, s) / d;
    let u = cross(q1 - p1, r) / d;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| p1 + r * t)
}

/// Whether segments p1-p2 and q1-q2 intersect.
fn segments_cross(p1: Pos2, p2: Pos2, q1: Pos2, q2: Pos2) -> bool {
    segment_intersection(p1, p2, q1, q2).is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_ids_encode_the_slot() {
        let l = LayerRef { stack: 7, index: 3 };
        let id = group_id(l).unwrap();
        assert!(is_virtual(id) && id != SINK);
        assert_eq!(group_of(id), Some(l));
        assert_eq!(group_of(SINK), None);
        assert_eq!(group_of(7), None);
        // Outside the encoding (docs/reference/limits.md): shown ungrouped.
        assert_eq!(group_id(LayerRef { stack: 1 << 19, index: 0 }), None);
        assert_eq!(group_id(LayerRef { stack: 1, index: 4096 }), None);
    }

    #[test]
    fn layers_are_shown_as_groups() {
        let mut g = Graph::new();
        let a = ops::add_fill_layer(&mut g, None);
        let b = ops::add_paint_layer(&mut g, Some(a));
        ops::add_mask(&mut g, a, false);
        let mut shown = BTreeSet::new();
        shown_from(&g, g.output_node().unwrap(), &mut shown);
        let (ga, gb) = (group_id(a).unwrap(), group_id(b).unwrap());
        let expected: BTreeSet<NodeId> = [g.output_node().unwrap(), g.root, ga, gb].into();
        assert_eq!(shown, expected, "no layer content or mask nodes");
        // The Stack's layer inputs are drawn from the groups' outputs.
        let links = shown_links(&g, None, g.root);
        assert!(links.contains(&(format!("slot{}", a.index), ga, "content".into())));
        assert!(links.contains(&(format!("slot{}.mask", a.index), ga, "mask".into())));
        let outs: Vec<String> = outputs(&g, ga).into_iter().map(|(o, _)| o).collect();
        assert_eq!(outs, ["content", "mask"]);
        assert_eq!(outputs(&g, gb).len(), 1, "no mask, no Mask output");
        // Moving a group moves the slot's saved position.
        set_pos(&mut g, ga, [-300.0, 40.0]);
        assert_eq!(ops::slot(&g, a).unwrap().node_pos, [-300.0, 40.0]);
    }

    #[test]
    fn input_values_take_the_input_rows() {
        let mut g = Graph::new();
        let m = g.add("math");
        // Math: Out, Op, then A and B (with its value field): b_value isn't a row of its own.
        assert_eq!(own_params(&g, m).iter().map(|p| p.name).collect::<Vec<_>>(), ["op"]);
        assert_eq!(input_row(&g, m, "a"), Some(2));
        assert_eq!(node_height(&g, m), HEADER + 4.0 * ROW + 6.0);
        // A Vector value takes four rows until something is connected.
        let vm = g.add("vector_math");
        let unlinked = node_height(&g, vm);
        let v = g.add("vector");
        g.connect(v, "vector", vm, "b");
        assert_eq!(unlinked - node_height(&g, vm), 3.0 * ROW);
        // Mapping's Vector value inputs take four rows each, below its own Vector input.
        let map = g.add("mapping");
        assert_eq!(param_rows(&g, map), 0);
        assert_eq!(node_height(&g, map), HEADER + 14.0 * ROW + 6.0);
        // Settings that depend on a choice are shown only while it has that value.
        let val = g.add("value");
        assert_eq!(own_params(&g, val).iter().map(|p| p.name).collect::<Vec<_>>(), ["type", "value"]);
        g.set(val, "type", Value::Text("Factor".into()));
        assert_eq!(own_params(&g, val).iter().map(|p| p.name).collect::<Vec<_>>(), ["type", "factor"]);
        assert_eq!(g.node(val).title(), "Factor");
    }

    #[test]
    fn reroutes_take_any_type_and_pass_it_on() {
        let mut g = Graph::new();
        let r = g.add("reroute");
        let out = |g: &Graph| outputs(g, r)[0].1;
        assert_eq!(out(&g), SocketType::Float, "unconnected");
        let fill = g.add("fill");
        assert!(accepts(&g, r, SocketType::Material, inputs(&g, r)[0].2));
        assert!(connect(&mut g, None, &SocketRef { node: fill, name: "out".into() }, &SocketRef { node: r, name: "in".into() }));
        assert_eq!(out(&g), SocketType::Material);
        let r2 = g.add("reroute");
        g.connect(r, "out", r2, "in");
        assert_eq!(outputs(&g, r2)[0].1, SocketType::Material, "through a chain");
        let c = g.add("vector");
        g.set(c, "type", Value::Text("Color".into()));
        assert_eq!(outputs(&g, c)[0].1, SocketType::Color);
        assert_eq!(segment_intersection(Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0), Pos2::new(0.0, 10.0), Pos2::new(10.0, 0.0)), Some(Pos2::new(5.0, 5.0)));
        assert_eq!(segment_intersection(Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(0.0, 1.0), Pos2::new(1.0, 1.0)), None);
    }

    #[test]
    fn frames_lose_deleted_nodes() {
        let mut g = Graph::new();
        let (a, b) = (g.add("noise"), g.add("levels"));
        g.frames.push(graph::Frame { label: "Frame".into(), nodes: [a, b].into() });
        g.remove(a);
        assert_eq!(g.frames[0].nodes, BTreeSet::from([b]));
        g.remove(b);
        assert!(g.frames.is_empty(), "empty frames are dropped");
        let f = frame_id(3);
        assert!(is_virtual(f) && frame_of(f) == Some(3) && group_of(f).is_none());
        assert_eq!(frame_of(group_id(LayerRef { stack: 9, index: 1 }).unwrap()), None);
    }

    #[test]
    fn tab_over_the_canvas_opens_the_add_menu_instead_of_moving_focus() {
        use egui::{Event, Key, Modifiers, RawInput};
        let ctx = egui::Context::default();
        let mut ed = NodeEditor { open: true, canvas_rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)), ..Default::default() };
        let field = egui::Id::new("field");
        let frame = |ed: &mut NodeEditor, events: Vec<Event>, k: f64| {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
                time: Some(k * 0.05),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                ed.intercept_tab(ui.ctx());
                // Two focusable controls, the first one focused.
                let a = ui.interact(Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::splat(20.0)), field, Sense::click());
                ui.interact(Rect::from_min_size(Pos2::new(30.0, 0.0), Vec2::splat(20.0)), field.with(1), Sense::click());
                if k == 0.0 {
                    a.request_focus();
                }
            });
            out.textures_delta.clear();
        };
        let tab = Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut ed, vec![Event::PointerMoved(Pos2::new(50.0, 50.0))], 0.0);
        frame(&mut ed, vec![tab.clone()], 1.0);
        assert!(ed.tab_pressed);
        frame(&mut ed, vec![], 2.0);
        assert_eq!(ctx.memory(|m| m.focused()), None, "the focus doesn't move on to the next control");
        // Away from the canvas, Tab is left to egui.
        ed.tab_pressed = false;
        frame(&mut ed, vec![Event::PointerMoved(Pos2::new(300.0, 150.0))], 3.0);
        frame(&mut ed, vec![tab], 4.0);
        assert!(!ed.tab_pressed);
    }
}
