//! Node graph data model. See Painter-docs/reference/nodes.md and Painter-docs/reference/layers.md.
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
    Material,
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
    /// Changing a structural parameter changes the generated shader code.
    pub structural: bool,
}

pub struct NodeDef {
    pub ty: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub inputs: &'static [(&'static str, SocketType)],
    pub outputs: &'static [(&'static str, SocketType)],
    pub params: Vec<ParamDef>,
    /// Filters read neighboring texels and are always cached.
    pub non_local: bool,
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

    /// Modes available per channel, see Painter-docs/reference/blend-modes.md.
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
}

impl Node {
    pub fn def(&self) -> &'static NodeDef {
        def(&self.ty).unwrap_or_else(|| def("constant").unwrap())
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
        if self.label.is_empty() { self.def().title } else { &self.label }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: BTreeMap<NodeId, Node>,
    pub next_id: NodeId,
    pub root: NodeId,
}

impl Graph {
    /// A graph with a Material Output fed by an empty root stack.
    pub fn new() -> Self {
        let mut g = Graph { nodes: BTreeMap::new(), next_id: 1, root: 0 };
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
    }

    /// Removes nodes that nothing reachable from the output references.
    pub fn collect_garbage(&mut self) -> Vec<Node> {
        let Some(out) = self.output_node() else { return Vec::new() };
        let live = self.subgraph(out);
        let dead: Vec<NodeId> = self.nodes.keys().copied().filter(|k| !live.contains(k)).collect();
        dead.into_iter().filter_map(|k| self.nodes.remove(&k)).collect()
    }

    /// Output socket type of `node.output`.
    pub fn output_type(&self, link: &Link) -> Option<SocketType> {
        let n = self.get(link.node)?;
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

fn f(v: f32) -> fn() -> Value {
    // Function pointers can't capture; use a small table of constructors instead.
    match v {
        _ if v == 0.0 => || Value::Float(0.0),
        _ if v == 1.0 => || Value::Float(1.0),
        _ if v == 0.5 => || Value::Float(0.5),
        _ => unreachable!(),
    }
}

macro_rules! p {
    ($name:expr, $kind:expr, $default:expr) => {
        ParamDef { name: $name, kind: $kind, default: || $default, structural: false }
    };
    ($name:expr, $kind:expr, $default:expr, structural) => {
        ParamDef { name: $name, kind: $kind, default: || $default, structural: true }
    };
}

use SocketType::{Color as C, Float as F, Material as M};

fn float(min: f32, max: f32) -> ParamKind {
    ParamKind::Float { min, max }
}

pub const PROJECTIONS: &[&str] = &["UV", "Triplanar"];
pub const NOISE_TYPES: &[&str] = &["Perlin", "Simplex", "Value"];
pub const AXES: &[&str] = &["X", "Y", "Z", "Custom"];
pub const SPACES: &[&str] = &["Object", "World"];
pub const MAP_NAMES: &[&str] = &["AO", "Curvature", "Thickness", "World Normal", "Position", "ID"];
pub const MATH_OPS: &[&str] =
    &["Add", "Subtract", "Multiply", "Divide", "Power", "Min", "Max", "Abs", "Smoothstep"];
pub const METRICS: &[&str] = &["Euclidean", "Manhattan"];
pub const CELL_OUTPUTS: &[&str] = &["F1", "F2", "F2-F1"];
pub const CURVATURE_SELECT: &[&str] = &["Convex", "Concave"];

/// Per-channel parameters of a Fill node, named `<field>_<param>`.
fn fill_params() -> Vec<ParamDef> {
    let mut v = vec![
        p!("projection", ParamKind::Enum(PROJECTIONS), Value::Text("UV".into()), structural),
        p!("scale", float(0.001, 1000.0), Value::Float(1.0)),
        p!("rotation", float(-360.0, 360.0), Value::Float(0.0)),
        p!("offset", ParamKind::Vec3, Value::Vec3([0.0; 3])),
        p!("triplanar_blend", float(0.0, 1.0), Value::Float(0.25)),
    ];
    for c in Channel::ALL {
        let (on, val, src, img): (&'static str, &'static str, &'static str, &'static str) = match c {
            Channel::BaseColor => ("base_color_on", "base_color", "base_color_source", "base_color_image"),
            Channel::Roughness => ("roughness_on", "roughness", "roughness_source", "roughness_image"),
            Channel::Metallic => ("metallic_on", "metallic", "metallic_source", "metallic_image"),
            Channel::Height => ("height_on", "height", "height_source", "height_image"),
            Channel::Normal => ("normal_on", "normal", "normal_source", "normal_image"),
            Channel::Emissive => ("emissive_on", "emissive", "emissive_source", "emissive_image"),
            Channel::Opacity => ("opacity_on", "opacity", "opacity_source", "opacity_image"),
        };
        v.push(ParamDef {
            name: on,
            kind: ParamKind::Bool,
            default: match c {
                Channel::Height | Channel::Normal | Channel::Emissive | Channel::Opacity => || Value::Bool(false),
                _ => || Value::Bool(true),
            },
            structural: false,
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
                Channel::Roughness => f(0.5),
                Channel::Opacity => f(1.0),
                _ => f(0.0),
            },
            structural: false,
        });
        v.push(ParamDef {
            name: src,
            kind: ParamKind::Enum(&["Constant", "Image"]),
            default: || Value::Text("Constant".into()),
            structural: true,
        });
        v.push(ParamDef { name: img, kind: ParamKind::Image, default: || Value::Text(String::new()), structural: true });
    }
    v
}

pub fn fill_param_names(c: Channel) -> (&'static str, &'static str, &'static str, &'static str) {
    let p = fill_params();
    // Order per channel: on, value, source, image; channels start after the 5 shared params.
    let base = 5 + c.index() * 4;
    (p[base].name, p[base + 1].name, p[base + 2].name, p[base + 3].name)
}

static DEFS: std::sync::LazyLock<Vec<NodeDef>> = std::sync::LazyLock::new(|| {
    use ParamKind::*;
    vec![
        // Sources
        NodeDef { ty: "paint", title: "Paint", category: "Sources", inputs: &[], outputs: &[("out", M)], params: vec![], non_local: false },
        NodeDef { ty: "paint_mask", title: "Paint Mask", category: "Sources", inputs: &[], outputs: &[("out", F)], params: vec![], non_local: false },
        NodeDef {
            ty: "constant", title: "Constant", category: "Sources", inputs: &[], outputs: &[("float", F), ("color", C)],
            params: vec![p!("value", float(0.0, 1.0), Value::Float(1.0)), p!("color", Color, Value::Vec3([0.5; 3]))],
            non_local: false,
        },
        NodeDef {
            ty: "image", title: "Image", category: "Sources", inputs: &[], outputs: &[("color", C), ("alpha", F)],
            params: vec![
                p!("image", Image, Value::Text(String::new()), structural),
                p!("srgb", Bool, Value::Bool(true)),
                p!("projection", Enum(PROJECTIONS), Value::Text("UV".into()), structural),
                p!("scale", float(0.001, 1000.0), Value::Float(1.0)),
                p!("rotation", float(-360.0, 360.0), Value::Float(0.0)),
                p!("offset", Vec3, Value::Vec3([0.0; 3])),
                p!("triplanar_blend", float(0.0, 1.0), Value::Float(0.25)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "mesh_map", title: "Mesh Map", category: "Sources", inputs: &[], outputs: &[("float", F), ("color", C)],
            params: vec![p!("map", Enum(MAP_NAMES), Value::Text("AO".into()), structural)],
            non_local: false,
        },
        // Procedural
        NodeDef {
            ty: "noise", title: "Noise", category: "Procedural", inputs: &[], outputs: &[("out", F)],
            params: vec![
                p!("type", Enum(NOISE_TYPES), Value::Text("Perlin".into()), structural),
                p!("scale", float(0.01, 1000.0), Value::Float(4.0)),
                p!("octaves", Int { min: 1, max: 10 }, Value::Float(5.0)),
                p!("lacunarity", float(1.0, 4.0), Value::Float(2.0)),
                p!("gain", float(0.0, 1.0), Value::Float(0.5)),
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "cellular", title: "Cellular", category: "Procedural", inputs: &[], outputs: &[("out", F)],
            params: vec![
                p!("scale", float(0.01, 1000.0), Value::Float(4.0)),
                p!("jitter", float(0.0, 1.0), Value::Float(1.0)),
                p!("metric", Enum(METRICS), Value::Text("Euclidean".into()), structural),
                p!("output", Enum(CELL_OUTPUTS), Value::Text("F1".into()), structural),
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "gradient", title: "Gradient", category: "Procedural", inputs: &[], outputs: &[("out", F)],
            params: vec![
                p!("axis", Enum(AXES), Value::Text("Y".into())),
                p!("custom_axis", Vec3, Value::Vec3([0.0, 1.0, 0.0])),
                p!("start", float(-1.0, 2.0), Value::Float(0.0)),
                p!("end", float(-1.0, 2.0), Value::Float(1.0)),
                p!("space", Enum(SPACES), Value::Text("Object".into())),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "id_select", title: "ID Select", category: "Procedural", inputs: &[], outputs: &[("out", F)],
            params: vec![p!("colors", Colors, Value::Colors(vec![]), structural), p!("tolerance", float(0.0, 1.0), Value::Float(0.1))],
            non_local: false,
        },
        // Generators
        NodeDef {
            ty: "edge_wear", title: "Edge Wear", category: "Generators", inputs: &[], outputs: &[("out", F)],
            params: vec![
                p!("amount", float(0.0, 1.0), Value::Float(0.2)),
                p!("sharpness", float(0.0, 1.0), Value::Float(0.6)),
                p!("noise_scale", float(0.1, 200.0), Value::Float(30.0)),
                p!("noise_amount", float(0.0, 1.0), Value::Float(0.7)),
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "dirt", title: "Dirt", category: "Generators", inputs: &[], outputs: &[("out", F)],
            params: vec![
                p!("amount", float(0.0, 1.0), Value::Float(0.5)),
                p!("contrast", float(0.0, 1.0), Value::Float(0.5)),
                p!("noise_scale", float(0.1, 200.0), Value::Float(8.0)),
                p!("seed", Int { min: 0, max: 9999 }, Value::Float(0.0)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "curvature_mask", title: "Curvature Mask", category: "Generators", inputs: &[], outputs: &[("out", F)],
            params: vec![
                p!("select", Enum(CURVATURE_SELECT), Value::Text("Convex".into())),
                p!("threshold", float(0.0, 1.0), Value::Float(0.1)),
                p!("softness", float(0.0, 1.0), Value::Float(0.2)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "position_gradient", title: "Position Gradient", category: "Generators", inputs: &[], outputs: &[("out", F)],
            params: vec![
                p!("axis", Enum(&["X", "Y", "Z"]), Value::Text("Y".into())),
                p!("falloff", float(0.0, 1.0), Value::Float(0.5)),
                p!("noise_amount", float(0.0, 1.0), Value::Float(0.2)),
            ],
            non_local: false,
        },
        // Adjust
        NodeDef {
            ty: "levels", title: "Levels", category: "Adjust", inputs: &[("in", C)], outputs: &[("out", C), ("float", F)],
            params: vec![
                p!("in_black", float(0.0, 1.0), Value::Float(0.0)),
                p!("in_white", float(0.0, 1.0), Value::Float(1.0)),
                p!("gamma", float(0.01, 10.0), Value::Float(1.0)),
                p!("out_black", float(0.0, 1.0), Value::Float(0.0)),
                p!("out_white", float(0.0, 1.0), Value::Float(1.0)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "ramp", title: "Ramp", category: "Adjust", inputs: &[("in", F)], outputs: &[("out", C)],
            params: vec![p!("stops", Stops, Value::Stops(vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])]), structural)],
            non_local: false,
        },
        NodeDef { ty: "invert", title: "Invert", category: "Adjust", inputs: &[("in", C)], outputs: &[("out", C), ("float", F)], params: vec![], non_local: false },
        NodeDef {
            ty: "clamp", title: "Clamp", category: "Adjust", inputs: &[("in", C)], outputs: &[("out", C), ("float", F)],
            params: vec![p!("min", float(-10.0, 10.0), Value::Float(0.0)), p!("max", float(-10.0, 10.0), Value::Float(1.0))],
            non_local: false,
        },
        NodeDef {
            ty: "hsv_adjust", title: "HSV Adjust", category: "Adjust", inputs: &[("in", C)], outputs: &[("out", C)],
            params: vec![
                p!("hue", float(-0.5, 0.5), Value::Float(0.0)),
                p!("saturation", float(0.0, 2.0), Value::Float(1.0)),
                p!("value", float(0.0, 2.0), Value::Float(1.0)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "math", title: "Math", category: "Adjust", inputs: &[("a", F), ("b", F)], outputs: &[("out", F)],
            params: vec![
                p!("op", Enum(MATH_OPS), Value::Text("Multiply".into()), structural),
                p!("b_value", float(-10.0, 10.0), Value::Float(1.0)),
            ],
            non_local: false,
        },
        // Combine
        NodeDef {
            ty: "merge", title: "Merge", category: "Combine", inputs: &[("a", C), ("b", C), ("mask", F)], outputs: &[("out", C), ("float", F)],
            params: vec![
                p!("mode", Enum(&["Normal", "Multiply", "Screen", "Overlay", "Add", "Subtract", "Lighten", "Darken", "Color Dodge", "Color Burn"]), Value::Text("Normal".into())),
                p!("opacity", float(0.0, 1.0), Value::Float(1.0)),
            ],
            non_local: false,
        },
        NodeDef {
            ty: "material_combine", title: "Material Combine", category: "Combine",
            inputs: &[("base_color", C), ("roughness", F), ("metallic", F), ("height", F), ("normal", C), ("emissive", C), ("opacity", F)],
            outputs: &[("out", M)],
            params: vec![],
            non_local: false,
        },
        NodeDef {
            ty: "material_split", title: "Material Split", category: "Combine", inputs: &[("in", M)],
            outputs: &[("base_color", C), ("roughness", F), ("metallic", F), ("height", F), ("normal", C), ("emissive", C), ("opacity", F)],
            params: vec![],
            non_local: false,
        },
        NodeDef { ty: "fill", title: "Fill", category: "Combine", inputs: &[], outputs: &[("out", M)], params: fill_params(), non_local: false },
        NodeDef { ty: "stack", title: "Stack", category: "Combine", inputs: &[], outputs: &[("out", M)], params: vec![], non_local: false },
        NodeDef { ty: "mask_stack", title: "Mask Stack", category: "Combine", inputs: &[], outputs: &[("out", F)], params: vec![], non_local: false },
        // Filters
        NodeDef {
            ty: "blur", title: "Blur", category: "Filters", inputs: &[("in", C)], outputs: &[("out", C), ("float", F)],
            params: vec![p!("radius", float(0.0, 64.0), Value::Float(4.0))],
            non_local: true,
        },
        NodeDef {
            ty: "dilate", title: "Dilate", category: "Filters", inputs: &[("in", F)], outputs: &[("out", F)],
            params: vec![p!("radius", float(0.0, 64.0), Value::Float(2.0))],
            non_local: true,
        },
        NodeDef {
            ty: "height_to_normal", title: "Height to Normal", category: "Filters", inputs: &[("in", F)], outputs: &[("out", C)],
            params: vec![p!("strength", float(0.0, 10.0), Value::Float(1.0))],
            non_local: true,
        },
        // Output
        NodeDef { ty: "material_output", title: "Material Output", category: "Output", inputs: &[("in", M)], outputs: &[], params: vec![], non_local: false },
    ]
});

pub fn defs() -> &'static [NodeDef] {
    &DEFS
}

pub fn def(ty: &str) -> Option<&'static NodeDef> {
    DEFS.iter().find(|d| d.ty == ty)
}

/// Node types offered by the "Add generator" menu.
pub const GENERATORS: &[&str] = &["edge_wear", "dirt", "curvature_mask", "position_gradient"];
