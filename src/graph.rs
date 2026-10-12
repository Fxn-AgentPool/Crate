//! Node graph data model. See docs/reference/nodes.md and docs/reference/layers.md.
//!
//! Nodes are generic: a type name, a parameter map and named input links. Static
//! `NodeDef`s describe each type's sockets and parameters, which drives the property UI,
//! serialization, parameter packing and code generation alike. Stack and Mask Stack nodes
//! additionally carry their ordered slots (the layers shown in the Layers panel).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::channels::Channel;

pub type NodeId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocketType {
    Float,
    Color,
    /// Three numbers: a coordinate, direction or offset (UVs, positions).
    Vector,
    Material,
}

/// Where a node is listed in the add menu, and its header color in the Node Editor.
/// See docs/reference/nodes.md "Categories".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Input,
    Texture,
    Generator,
    Color,
    Vector,
    Converter,
    Filter,
    Layer,
    Layout,
    Output,
}

impl Category {
    /// In add menu order.
    pub const ALL: [Category; 10] = [
        Category::Input,
        Category::Texture,
        Category::Generator,
        Category::Color,
        Category::Vector,
        Category::Converter,
        Category::Filter,
        Category::Layer,
        Category::Layout,
        Category::Output,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Input => "Input",
            Category::Texture => "Texture",
            Category::Generator => "Generator",
            Category::Color => "Color",
            Category::Vector => "Vector",
            Category::Converter => "Converter",
            Category::Filter => "Filter",
            Category::Layer => "Layer",
            Category::Layout => "Layout",
            Category::Output => "Output",
        }
    }

    /// Header color, as sRGB.
    pub fn color(self) -> [u8; 3] {
        match self {
            Category::Input => [0x83, 0x31, 0x4a],
            Category::Texture => [0x79, 0x46, 0x1d],
            Category::Generator => [0x1f, 0x70, 0x50],
            Category::Color => [0x6c, 0x6c, 0x00],
            Category::Vector => [0x3c, 0x3c, 0x83],
            Category::Converter => [0x24, 0x62, 0x83],
            Category::Filter => [0x6c, 0x3b, 0x75],
            Category::Layer => [0x2d, 0x6a, 0x2d],
            Category::Layout => [0x4a, 0x4d, 0x55],
            Category::Output => [0x4d, 0x00, 0x17],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Float(f32),
    Vec3([f32; 3]),
    Text(String),
    /// Ramp stops: (position, rgb).
    Stops(Vec<(f32, [f32; 3])>),
    /// ID colors for ID Select.
    Colors(Vec<[f32; 3]>),
}

impl Value {
    pub fn as_f32(&self) -> f32 {
        match self {
            Value::Bool(b) => *b as u8 as f32,
            Value::Float(f) => *f,
            Value::Vec3(v) => v[0],
            _ => 0.0,
        }
    }
    pub fn as_vec3(&self) -> [f32; 3] {
        match self {
            Value::Vec3(v) => *v,
            Value::Float(f) => [*f; 3],
            _ => [0.0; 3],
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Float(f) => *f != 0.0,
            _ => false,
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            Value::Text(s) => s,
            _ => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    Float { min: f32, max: f32 },
    /// Integer stored as float.
    Int { min: i32, max: i32 },
    /// Linear RGB.
    Color,
    Vec3,
    Bool,
    Enum(&'static [&'static str]),
    /// Image name in the project's images directory ("" = none).
    Image,
    Stops,
    Colors,
}

#[derive(Clone, Debug)]
pub struct ParamDef {
    pub name: &'static str,
    pub kind: ParamKind,
    pub default: fn() -> Value,
    /// A value input: also an input socket of the same name, whose link replaces the value.
    pub socket: bool,
    /// Shown only while this other (choice) parameter has this value.
    pub when: Option<(&'static str, &'static str)>,
}

impl ParamDef {
    fn when(mut self, param: &'static str, value: &'static str) -> Self {
        self.when = Some((param, value));
        self
    }

    /// The socket type of a value input.
    pub fn socket_type(&self) -> SocketType {
        match self.kind {
            ParamKind::Color => SocketType::Color,
            ParamKind::Vec3 => SocketType::Vector,
            _ => SocketType::Float,
        }
    }
}

pub struct NodeDef {
    pub ty: &'static str,
    pub title: &'static str,
    pub category: Category,
    pub inputs: &'static [(&'static str, SocketType)],
    pub outputs: &'static [(&'static str, SocketType)],
    pub params: Vec<ParamDef>,
    /// Filters read neighboring texels and are always cached.
    pub non_local: bool,
}

impl NodeDef {
    /// Every input socket: the link-only inputs, then the value inputs.
    /// See docs/reference/nodes.md "Inputs and settings".
    pub fn input_sockets(&self) -> Vec<(&'static str, SocketType)> {
        let mut v = self.inputs.to_vec();
        v.extend(self.params.iter().filter(|p| p.socket).map(|p| (p.name, p.socket_type())));
        v
    }

    /// The value an input uses while nothing is connected to it.
    pub fn input_param(&self, input: &str) -> Option<&ParamDef> {
        self.params.iter().find(|p| p.socket && p.name == input)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub node: NodeId,
    pub output: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
    Subtract,
    Lighten,
    Darken,
    ColorDodge,
    ColorBurn,
    Max,
    Min,
    /// Normal channel: reoriented normal mapping.
    Combine,
}

impl BlendMode {
    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Add => "Add",
            BlendMode::Subtract => "Subtract",
            BlendMode::Lighten => "Lighten",
            BlendMode::Darken => "Darken",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::Max => "Max",
            BlendMode::Min => "Min",
            BlendMode::Combine => "Combine",
        }
    }

    /// Modes available per channel, see docs/reference/blend-modes.md.
    pub fn for_channel(c: Channel) -> &'static [BlendMode] {
        use BlendMode::*;
        match c {
            Channel::Height => &[Normal, Add, Subtract, Max, Min, Multiply],
            Channel::Normal => &[Normal, Combine],
            _ => &[Normal, Multiply, Screen, Overlay, Add, Subtract, Lighten, Darken, ColorDodge, ColorBurn],
        }
    }

    pub const MASK: &'static [BlendMode] = &[
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::Overlay,
    ];

    pub fn code(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelBlend {
    pub enabled: bool,
    pub mode: BlendMode,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub content: Option<NodeId>,
    /// A Mask Stack node.
    pub mask: Option<NodeId>,
    pub channels: [ChannelBlend; 7],
    /// Library name if this layer was instanced from a smart material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart_material: Option<String>,
    /// Expanded in the Layers panel (UI state).
    #[serde(default)]
    pub expanded: bool,
    /// Position of the layer's group in the Node Editor; [0, 0] until it is first placed.
    #[serde(default)]
    pub node_pos: [f32; 2],
}

impl Slot {
    pub fn new(name: impl Into<String>, content: NodeId) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            content: Some(content),
            mask: None,
            channels: Channel::ALL.map(|c| ChannelBlend {
                enabled: true,
                mode: if c == Channel::Normal { BlendMode::Combine } else { BlendMode::Normal },
                opacity: 1.0,
            }),
            smart_material: None,
            expanded: false,
            node_pos: [0.0, 0.0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MaskSource {
    /// A node producing a Float: Paint Mask, Fill (constant/image) or a generator graph.
    Node(NodeId),
    /// Adjusts the accumulated mask below it.
    Levels { in_black: f32, in_white: f32, gamma: f32, out_black: f32, out_white: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskSlot {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub mode: BlendMode,
    pub source: MaskSource,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Link>,
    #[serde(default)]
    pub cached: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub exposed: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Slot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mask_slots: Vec<MaskSlot>,
    /// Node Editor position.
    #[serde(default)]
    pub pos: [f32; 2],
    /// Collapsed to its header in the Node Editor.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
}

impl Node {
    pub fn def(&self) -> &'static NodeDef {
        def(&self.ty).unwrap_or_else(|| def("value").unwrap())
    }

    pub fn param(&self, name: &str) -> Value {
        self.params.get(name).cloned().unwrap_or_else(|| {
            self.def()
                .params
                .iter()
                .find(|p| p.name == name)
                .map(|p| (p.default)())
                .unwrap_or(Value::Float(0.0))
        })
    }

    pub fn title(&self) -> &str {
        if !self.label.is_empty() {
            return &self.label;
        }
        // Constants are named after their type.
        match (self.ty.as_str(), self.param("type").as_str()) {
            ("value", "Factor") => "Factor",
            ("vector", "Color") => "Color",
            _ => self.def().title,
        }
    }

    /// A Value node's number, after its type.
    pub fn value(&self) -> f32 {
        let name = if self.param("type").as_str() == "Factor" { "factor" } else { "value" };
        self.param(name).as_f32()
    }

    /// Whether a parameter applies, given the choices it depends on.
    pub fn shows(&self, p: &ParamDef) -> bool {
        p.when.is_none_or(|(other, value)| self.param(other).as_str() == value)
    }
}

/// A Node Editor frame: a labeled box around some nodes. See docs/reference/node-editor.md
/// "Frames".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub label: String,
    pub nodes: BTreeSet<NodeId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: BTreeMap<NodeId, Node>,
    pub next_id: NodeId,
    pub root: NodeId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
}

impl Graph {
    /// A graph with a Material Output fed by an empty root stack.
    pub fn new() -> Self {
        let mut g = Graph { nodes: BTreeMap::new(), next_id: 1, root: 0, frames: Vec::new() };
        let root = g.add("stack");
        let out = g.add("material_output");
        g.connect(root, "out", out, "in");
        g.root = root;
        g.node_mut(root).pos = [-260.0, 0.0];
        g
    }

    pub fn add(&mut self, ty: &str) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        let d = def(ty).unwrap_or_else(|| panic!("unknown node type {ty}"));
        let params = d.params.iter().map(|p| (p.name.to_string(), (p.default)())).collect();
        self.nodes.insert(
            id,
            Node {
                id,
                ty: ty.to_string(),
                label: String::new(),
                params,
                inputs: BTreeMap::new(),
                cached: d.non_local,
                exposed: BTreeSet::new(),
                slots: Vec::new(),
                mask_slots: Vec::new(),
                pos: [0.0, 0.0],
                collapsed: false,
            },
        );
        id
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[&id]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.nodes.get_mut(&id).expect("node exists")
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn output_node(&self) -> Option<NodeId> {
        self.nodes.values().find(|n| n.ty == "material_output").map(|n| n.id)
    }

    pub fn set(&mut self, id: NodeId, name: &str, value: Value) {
        self.node_mut(id).params.insert(name.to_string(), value);
    }

    /// Connects `from.output` to `to.input`. Rejects connections that would create a cycle.
    pub fn connect(&mut self, from: NodeId, output: &str, to: NodeId, input: &str) -> bool {
        if from == to || self.depends_on(from, to) {
            return false;
        }
        self.node_mut(to).inputs.insert(input.to_string(), Link { node: from, output: output.to_string() });
        true
    }

    pub fn disconnect(&mut self, to: NodeId, input: &str) {
        self.node_mut(to).inputs.remove(input);
    }

    /// Every node `id` references: input links, slot contents and masks.
    pub fn dependencies(&self, id: NodeId) -> Vec<NodeId> {
        let Some(n) = self.get(id) else { return Vec::new() };
        let mut deps: Vec<NodeId> = n.inputs.values().map(|l| l.node).collect();
        for s in &n.slots {
            deps.extend(s.content);
            deps.extend(s.mask);
        }
        for m in &n.mask_slots {
            if let MaskSource::Node(c) = m.source {
                deps.push(c);
            }
        }
        deps
    }

    /// True if `a` (transitively) reads from `b`.
    pub fn depends_on(&self, a: NodeId, b: NodeId) -> bool {
        let mut stack = vec![a];
        let mut seen = BTreeSet::new();
        while let Some(n) = stack.pop() {
            if n == b {
                return true;
            }
            if seen.insert(n) {
                stack.extend(self.dependencies(n));
            }
        }
        false
    }

    /// All nodes reachable from `id`, including itself.
    pub fn subgraph(&self, id: NodeId) -> BTreeSet<NodeId> {
        let mut stack = vec![id];
        let mut seen = BTreeSet::new();
        while let Some(n) = stack.pop() {
            if self.nodes.contains_key(&n) && seen.insert(n) {
                stack.extend(self.dependencies(n));
            }
        }
        seen
    }

    /// Removes `id` and every connection and slot reference to it.
    pub fn remove(&mut self, id: NodeId) {
        self.nodes.remove(&id);
        for n in self.nodes.values_mut() {
            n.inputs.retain(|_, l| l.node != id);
            for s in &mut n.slots {
                if s.content == Some(id) {
                    s.content = None;
                }
                if s.mask == Some(id) {
                    s.mask = None;
                }
            }
            n.mask_slots.retain(|m| m.source != MaskSource::Node(id));
        }
        self.prune_frames();
    }

    /// Drops frame members that no longer exist, then empty frames.
    pub fn prune_frames(&mut self) {
        let nodes = &self.nodes;
        for f in &mut self.frames {
            f.nodes.retain(|n| nodes.contains_key(n));
        }
        self.frames.retain(|f| !f.nodes.is_empty());
    }

    /// Removes nodes that nothing reachable from the output references.
    pub fn collect_garbage(&mut self) -> Vec<Node> {
        let Some(out) = self.output_node() else { return Vec::new() };
        let live = self.subgraph(out);
        let dead: Vec<NodeId> = self.nodes.keys().copied().filter(|k| !live.contains(k)).collect();
        let removed = dead.into_iter().filter_map(|k| self.nodes.remove(&k)).collect();
        self.prune_frames();
        removed
    }

    /// Output socket type of `node.output`.
    /// The type of an output. A Reroute's is the type of what it reads (Float when nothing), a
    /// Vector node's Color while its type is Color.
    pub fn output_type(&self, link: &Link) -> Option<SocketType> {
        let mut n = self.get(link.node)?;
        // Through chains of reroutes (bounded, in case a file holds a cycle).
        for _ in 0..256 {
            if n.ty != "reroute" {
                break;
            }
            match n.inputs.get("in") {
                Some(l) => {
                    let next = self.get(l.node)?;
                    if next.ty != "reroute" {
                        return self.output_type(l);
                    }
                    n = next;
                }
                None => return Some(SocketType::Float),
            }
        }
        if n.ty == "vector" && n.param("type").as_str() == "Color" {
            return Some(SocketType::Color);
        }
        n.def().outputs.iter().find(|(name, _)| *name == link.output).map(|(_, t)| *t)
    }

    /// Copies the subgraph rooted at `id` with fresh ids. Returns the new root and a map
    /// from old to new ids (used to duplicate paint data).
    pub fn duplicate(&mut self, id: NodeId) -> (NodeId, BTreeMap<NodeId, NodeId>) {
        let nodes = self.subgraph(id);
        let mut map = BTreeMap::new();
        for &old in &nodes {
            map.insert(old, self.next_id);
            self.next_id += 1;
        }
        for &old in &nodes {
            let mut n = self.nodes[&old].clone();
            n.id = map[&old];
            for l in n.inputs.values_mut() {
                l.node = *map.get(&l.node).unwrap_or(&l.node);
            }
            for s in &mut n.slots {
                s.content = s.content.map(|c| *map.get(&c).unwrap_or(&c));
                s.mask = s.mask.map(|c| *map.get(&c).unwrap_or(&c));
            }
            for m in &mut n.mask_slots {
                if let MaskSource::Node(c) = &mut m.source {
                    *c = *map.get(c).unwrap_or(c);
                }
            }
            n.pos = [n.pos[0] + 40.0, n.pos[1] + 40.0];
            self.nodes.insert(n.id, n);
        }
        (map[&id], map)
    }
}

// ------------------------------------------------------------------ node definitions

macro_rules! p {
    ($name:expr, $kind:expr, $default:expr) => {
        ParamDef { name: $name, kind: $kind, default: || $default, socket: false, when: None }
    };
}

/// A value input (a parameter that is also a socket).
macro_rules! s {
    ($name:expr, $kind:expr, $default:expr) => {
        ParamDef { name: $name, kind: $kind, default: || $default, socket: true, when: None }
    };
}

use SocketType::{Color as C, Float as F, Material as M, Vector as V};

fn float(min: f32, max: f32) -> ParamKind {
    ParamKind::Float { min, max }
}

pub const PROJECTIONS: &[&str] = &["UV", "Triplanar"];
pub const NOISE_TYPES: &[&str] = &["Perlin", "Simplex", "Value"];
pub const AXES: &[&str] = &["X", "Y", "Z", "Custom"];
pub const SPACES: &[&str] = &["Object", "World"];
pub const MAP_NAMES: &[&str] = &["AO", "Curvature", "Thickness", "World Normal", "Position", "ID", "Normal", "Height"];
pub const MATH_OPS: &[&str] =
    &["Add", "Subtract", "Multiply", "Divide", "Power", "Min", "Max", "Abs", "Smoothstep"];
pub const METRICS: &[&str] = &["Euclidean", "Manhattan"];
pub const CELL_OUTPUTS: &[&str] = &["F1", "F2", "F2-F1"];
pub const CURVATURE_SELECT: &[&str] = &["Convex", "Concave"];
pub const VALUE_TYPES: &[&str] = &["Float", "Factor"];
pub const VECTOR_TYPES: &[&str] = &["Vector", "Color"];
pub const VECTOR_OPS: &[&str] = &[
    "Add", "Subtract", "Multiply", "Divide", "Scale", "Dot Product", "Cross Product", "Length", "Distance", "Normalize",
    "Absolute", "Floor", "Fraction", "Minimum", "Maximum",
];

/// Per-channel parameters of a Fill node, named `<field>_<param>`.
fn fill_params() -> Vec<ParamDef> {
    let mut v = vec![
        p!("projection", ParamKind::Enum(PROJECTIONS), Value::Text("UV".into())),
        p!("scale", float(0.001, 1000.0), Value::Float(1.0)),
        p!("rotation", float(-360.0, 360.0), Value::Float(0.0)),
        p!("offset", ParamKind::Vec3, Value::Vec3([0.0; 3])),
        p!("triplanar_blend", float(0.0, 1.0), Value::Float(0.25)),
    ];
    for c in Channel::ALL {
        let (on, val, src, img) = fill_param_names(c);
        v.push(ParamDef {
            name: on,
            kind: ParamKind::Bool,
            default: match c {
                Channel::Height | Channel::Normal | Channel::Emissive | Channel::Opacity => || Value::Bool(false),
                _ => || Value::Bool(true),
            },
            socket: false,
            when: None,
        });
        v.push(ParamDef {
            name: val,
            kind: match c {
                Channel::BaseColor | Channel::Emissive => ParamKind::Color,
                Channel::Normal => ParamKind::Vec3,
                Channel::Height => float(-1.0, 1.0),
                _ => float(0.0, 1.0),
            },
            default: match c {
                Channel::BaseColor => || Value::Vec3([0.5, 0.5, 0.5]),
                Channel::Normal => || Value::Vec3([0.5, 0.5, 1.0]),
                Channel::Emissive => || Value::Vec3([0.0; 3]),
                Channel::Roughness => || Value::Float(0.5),
                Channel::Opacity => || Value::Float(1.0),
                _ => || Value::Float(0.0),
            },
            socket: false,
            when: None,
        });
        v.push(p!(src, ParamKind::Enum(&["Constant", "Image"]), Value::Text("Constant".into())));
        v.push(p!(img, ParamKind::Image, Value::Text(String::new())));
    }
    v
}

/// A Fill node's parameters for a channel: (on, value, source, image).
pub fn fill_param_names(c: Channel) -> (&'static str, &'static str, &'static str, &'static str) {
    match c {
        Channel::BaseColor => ("base_color_on", "base_color", "base_color_source", "base_color_image"),
        Channel::Roughness => ("roughness_on", "roughness", "roughness_source", "roughness_image"),
        Channel::Metallic => ("metallic_on", "metallic", "metallic_source", "metallic_image"),
        Channel::Height => ("height_on", "height", "height_source", "height_image"),
        Channel::Normal => ("normal_on", "normal", "normal_source", "normal_image"),
        Channel::Emissive => ("emissive_on", "emissive", "emissive_source", "emissive_image"),
        Channel::Opacity => ("opacity_on", "opacity", "opacity_source", "opacity_image"),
    }
}

static DEFS: std::sync::LazyLock<Vec<NodeDef>> = std::sync::LazyLock::new(|| {
    use Category as K;
    use ParamKind::*;
    type Sockets = &'static [(&'static str, SocketType)];
    let def = |ty: &'static str, title: &'static str, category: Category, inputs: Sockets, outputs: Sockets, params: Vec<ParamDef>| NodeDef {
        ty,
        title,
        category,
        inputs,
        outputs,
        params,
        non_local: false,
    };
    vec![
        // Input
        def(
            "value", "Value", K::Input, &[], &[("value", F)],
            vec![
                p!("type", Enum(VALUE_TYPES), Value::Text("Float".into())),
                p!("value", float(-1.0e6, 1.0e6), Value::Float(0.5)).when("type", "Float"),
                p!("factor", float(0.0, 1.0), Value::Float(1.0)).when("type", "Factor"),
            ],
        ),
        def(
            "vector", "Vector", K::Input, &[], &[("vector", V)],
            vec![
                p!("type", Enum(VECTOR_TYPES), Value::Text("Vector".into())),
                p!("vector", Vec3, Value::Vec3([0.0; 3])).when("type", "Vector"),
                p!("color", Color, Value::Vec3([0.5; 3])).when("type", "Color"),
            ],
        ),
        def("uv", "UV", K::Input, &[], &[("uv", V)], vec![]),
        def("geometry", "Geometry", K::Input, &[], &[("position", V), ("generated", V), ("normal", V)], vec![]),
        def("mesh_map", "Mesh Map", K::Input, &[], &[("float", F), ("color", C)], vec![p!("map", Enum(MAP_NAMES), Value::Text("AO".into()))]),
        def(
            "id_select", "ID Select", K::Input, &[], &[("out", F)],
            vec![p!("colors", Colors, Value::Colors(vec![])), s!("tolerance", float(0.0, 1.0), Value::Float(0.1))],
        ),
        // Texture
        def(
            "image", "Image", K::Texture, &[("vector", V)], &[("color", C), ("alpha", F)],
            vec![
                p!("image", Image, Value::Text(String::new())),
                p!("srgb", Bool, Value::Bool(true)),
                p!("projection", Enum(PROJECTIONS), Value::Text("UV".into())),
                s!("scale", float(0.001, 1000.0), Value::Float(1.0)),
                s!("rotation", float(-360.0, 360.0), Value::Float(0.0)),
                s!("offset", Vec3, Value::Vec3([0.0; 3])),
                s!("triplanar_blend", float(0.0, 1.0), Value::Float(0.25)),
            ],
        ),
        def(
            "noise", "Noise", K::Texture, &[("vector", V)], &[("out", F)],
            vec![
                p!("type", Enum(NOISE_TYPES), Value::Text("Perlin".into())),
                p!("octaves", Int { min: 1, max: 10 }, Value::Float(5.0)),
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
                s!("scale", float(0.01, 1000.0), Value::Float(4.0)),
                s!("lacunarity", float(1.0, 4.0), Value::Float(2.0)),
                s!("gain", float(0.0, 1.0), Value::Float(0.5)),
            ],
        ),
        def(
            "cellular", "Cellular", K::Texture, &[("vector", V)], &[("out", F)],
            vec![
                p!("metric", Enum(METRICS), Value::Text("Euclidean".into())),
                p!("output", Enum(CELL_OUTPUTS), Value::Text("F1".into())),
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
                s!("scale", float(0.01, 1000.0), Value::Float(4.0)),
                s!("jitter", float(0.0, 1.0), Value::Float(1.0)),
            ],
        ),
        def(
            "gradient", "Gradient", K::Texture, &[("vector", V)], &[("out", F)],
            vec![
                p!("axis", Enum(AXES), Value::Text("Y".into())),
                p!("custom_axis", Vec3, Value::Vec3([0.0, 1.0, 0.0])).when("axis", "Custom"),
                p!("space", Enum(SPACES), Value::Text("Object".into())),
                s!("start", float(-1.0, 2.0), Value::Float(0.0)),
                s!("end", float(-1.0, 2.0), Value::Float(1.0)),
            ],
        ),
        // Generator
        def(
            "edge_wear", "Edge Wear", K::Generator, &[], &[("out", F)],
            vec![
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
                s!("amount", float(0.0, 1.0), Value::Float(0.2)),
                s!("sharpness", float(0.0, 1.0), Value::Float(0.6)),
                s!("noise_scale", float(0.1, 200.0), Value::Float(30.0)),
                s!("noise_amount", float(0.0, 1.0), Value::Float(0.7)),
            ],
        ),
        def(
            "dirt", "Dirt", K::Generator, &[], &[("out", F)],
            vec![
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
                s!("amount", float(0.0, 1.0), Value::Float(0.5)),
                s!("contrast", float(0.0, 1.0), Value::Float(0.5)),
                s!("noise_scale", float(0.1, 200.0), Value::Float(8.0)),
            ],
        ),
        def(
            "curvature_mask", "Curvature Mask", K::Generator, &[], &[("out", F)],
            vec![
                p!("select", Enum(CURVATURE_SELECT), Value::Text("Convex".into())),
                s!("threshold", float(0.0, 1.0), Value::Float(0.1)),
                s!("softness", float(0.0, 1.0), Value::Float(0.2)),
            ],
        ),
        def(
            "position_gradient", "Position Gradient", K::Generator, &[], &[("out", F)],
            vec![
                p!("axis", Enum(&["X", "Y", "Z"]), Value::Text("Y".into())),
                s!("falloff", float(0.0, 1.0), Value::Float(0.5)),
                s!("noise_amount", float(0.0, 1.0), Value::Float(0.2)),
            ],
        ),
        // Color
        def(
            "levels", "Levels", K::Color, &[("in", C)], &[("out", C), ("float", F)],
            vec![
                s!("in_black", float(0.0, 1.0), Value::Float(0.0)),
                s!("in_white", float(0.0, 1.0), Value::Float(1.0)),
                s!("gamma", float(0.01, 10.0), Value::Float(1.0)),
                s!("out_black", float(0.0, 1.0), Value::Float(0.0)),
                s!("out_white", float(0.0, 1.0), Value::Float(1.0)),
            ],
        ),
        def("invert", "Invert", K::Color, &[("in", C)], &[("out", C), ("float", F)], vec![]),
        def(
            "hsv_adjust", "HSV Adjust", K::Color, &[("in", C)], &[("out", C)],
            vec![
                s!("hue", float(-0.5, 0.5), Value::Float(0.0)),
                s!("saturation", float(0.0, 2.0), Value::Float(1.0)),
                s!("value", float(0.0, 2.0), Value::Float(1.0)),
            ],
        ),
        def(
            "merge", "Merge", K::Color, &[("a", C), ("b", C), ("mask", F)], &[("out", C), ("float", F)],
            vec![
                p!("mode", Enum(&["Normal", "Multiply", "Screen", "Overlay", "Add", "Subtract", "Lighten", "Darken", "Color Dodge", "Color Burn"]), Value::Text("Normal".into())),
                s!("opacity", float(0.0, 1.0), Value::Float(1.0)),
            ],
        ),
        // Vector
        def(
            "uv_transform", "UV Transform", K::Vector, &[("vector", V)], &[("vector", V)],
            vec![
                s!("tile_u", float(0.0, 1000.0), Value::Float(1.0)),
                s!("tile_v", float(0.0, 1000.0), Value::Float(1.0)),
                s!("rotation", float(-360.0, 360.0), Value::Float(0.0)),
                s!("offset_u", float(-1000.0, 1000.0), Value::Float(0.0)),
                s!("offset_v", float(-1000.0, 1000.0), Value::Float(0.0)),
            ],
        ),
        def(
            "mapping", "Mapping", K::Vector, &[("vector", V)], &[("vector", V)],
            vec![
                s!("location", Vec3, Value::Vec3([0.0; 3])),
                s!("rotation", Vec3, Value::Vec3([0.0; 3])),
                s!("scale", Vec3, Value::Vec3([1.0; 3])),
            ],
        ),
        def(
            "vector_math", "Vector Math", K::Vector, &[("a", V)], &[("vector", V), ("value", F)],
            vec![
                p!("op", Enum(VECTOR_OPS), Value::Text("Add".into())),
                s!("b", Vec3, Value::Vec3([0.0; 3])),
                s!("scale", float(-1.0e6, 1.0e6), Value::Float(1.0)),
            ],
        ),
        // Converter
        def(
            "math", "Math", K::Converter, &[("a", F)], &[("out", F)],
            vec![p!("op", Enum(MATH_OPS), Value::Text("Multiply".into())), s!("b", float(-1.0e6, 1.0e6), Value::Float(1.0))],
        ),
        def(
            "clamp", "Clamp", K::Converter, &[("in", C)], &[("out", C), ("float", F)],
            vec![s!("min", float(-1.0e6, 1.0e6), Value::Float(0.0)), s!("max", float(-1.0e6, 1.0e6), Value::Float(1.0))],
        ),
        def(
            "ramp", "Ramp", K::Converter, &[("in", F)], &[("out", C)],
            vec![p!("stops", Stops, Value::Stops(vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])]))],
        ),
        def(
            "combine_xyz", "Combine XYZ", K::Converter, &[], &[("vector", V)],
            vec![
                s!("x", float(-1.0e6, 1.0e6), Value::Float(0.0)),
                s!("y", float(-1.0e6, 1.0e6), Value::Float(0.0)),
                s!("z", float(-1.0e6, 1.0e6), Value::Float(0.0)),
            ],
        ),
        def("separate_xyz", "Separate XYZ", K::Converter, &[("vector", V)], &[("x", F), ("y", F), ("z", F)], vec![]),
        def(
            "material_combine", "Material Combine", K::Converter,
            &[("base_color", C), ("roughness", F), ("metallic", F), ("height", F), ("normal", C), ("emissive", C), ("opacity", F)],
            &[("out", M)],
            vec![],
        ),
        def(
            "material_split", "Material Split", K::Converter, &[("in", M)],
            &[("base_color", C), ("roughness", F), ("metallic", F), ("height", F), ("normal", C), ("emissive", C), ("opacity", F)],
            vec![],
        ),
        // Filter
        NodeDef {
            ty: "blur", title: "Blur", category: K::Filter, inputs: &[("in", C)], outputs: &[("out", C), ("float", F)],
            params: vec![p!("radius", float(0.0, 64.0), Value::Float(4.0))],
            non_local: true,
        },
        NodeDef {
            ty: "dilate", title: "Dilate", category: K::Filter, inputs: &[("in", F)], outputs: &[("out", F)],
            params: vec![p!("radius", float(0.0, 64.0), Value::Float(2.0))],
            non_local: true,
        },
        NodeDef {
            ty: "height_to_normal", title: "Height to Normal", category: K::Filter, inputs: &[("in", F)], outputs: &[("out", C)],
            params: vec![p!("strength", float(0.0, 10.0), Value::Float(1.0))],
            non_local: true,
        },
        // Layer
        def("paint", "Paint", K::Layer, &[], &[("out", M)], vec![]),
        def("paint_mask", "Paint Mask", K::Layer, &[], &[("out", F)], vec![]),
        // Read at their own texel between UV islands (docs/reference/2d-paint.md).
        def("paint_2d", "2D Paint", K::Layer, &[], &[("out", M)], vec![]),
        def("paint_mask_2d", "2D Paint Mask", K::Layer, &[], &[("out", F)], vec![]),
        def("fill", "Fill", K::Layer, &[], &[("out", M)], fill_params()),
        def("stack", "Stack", K::Layer, &[], &[("out", M)], vec![]),
        def("mask_stack", "Mask Stack", K::Layer, &[], &[("out", F)], vec![]),
        // Layout. Its socket types follow what it's connected to (Graph::output_type).
        def("reroute", "Reroute", K::Layout, &[("in", F)], &[("out", F)], vec![]),
        // Output
        def("material_output", "Material Output", K::Output, &[("in", M)], &[], vec![]),
    ]
});

pub fn defs() -> &'static [NodeDef] {
    &DEFS
}

pub fn def(ty: &str) -> Option<&'static NodeDef> {
    DEFS.iter().find(|d| d.ty == ty)
}

/// Paint and 2D Paint: material paint nodes, painted into the material pools.
pub fn is_paint(ty: &str) -> bool {
    matches!(ty, "paint" | "paint_2d")
}

/// Paint Mask and 2D Paint Mask: single-channel paint nodes (mask pool).
pub fn is_paint_mask(ty: &str) -> bool {
    matches!(ty, "paint_mask" | "paint_mask_2d")
}

/// 2D Paint and 2D Paint Mask (docs/reference/2d-paint.md).
pub fn is_2d(ty: &str) -> bool {
    matches!(ty, "paint_2d" | "paint_mask_2d")
}

impl Graph {
    /// Whether `id` reads a 2D paint node, itself included. Such nodes aren't cached
    /// (docs/reference/2d-paint.md#caching).
    pub fn reads_2d(&self, id: NodeId) -> bool {
        self.subgraph(id).iter().any(|n| self.get(*n).is_some_and(|n| is_2d(&n.ty)))
    }
}

/// Node types offered by the "Add generator" menu.
pub const GENERATORS: &[&str] = &["edge_wear", "dirt", "curvature_mask", "position_gradient"];
