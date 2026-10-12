//! Compiles the node graph into WGSL. See docs/explanation/about-realtime-evaluation.md.
//!
//! Every Stack node becomes a function `stack_N(s, below) -> Mat`, every Mask Stack node
//! `mstack_N(s) -> f32`, and `eval_material` evaluates the Material Output. Other nodes are
//! inlined as `let` bindings into whichever function reads them. Parameters live in a
//! storage buffer, so editing a value only rewrites the buffer; the shader is recompiled
//! only when the generated code (the graph's structure) changes.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::channels::{Channel, ChannelSettings};
use crate::graph::{
    self, BlendMode, Graph, Link, MaskSource, Node, NodeId, ParamKind, SocketType, Value,
};

pub const LIB_WGSL: &str = concat!(include_str!("shaders/noise.wgsl"), include_str!("shaders/material_lib.wgsl"));

pub struct Compiled {
    /// Generated functions, including `eval_material`. Prepend `LIB_WGSL`.
    pub code: String,
    pub params: Vec<[f32; 4]>,
    pub errors: Vec<String>,
}

const MAX_STOPS: usize = 8;
const MAX_ID_COLORS: usize = 16;

/// Parameter buffer layout: channel defaults, then every node's parameters in a fixed
/// order, then per-slot data.
struct Layout {
    params: Vec<[f32; 4]>,
    node_param: HashMap<(NodeId, &'static str), usize>,
    slot: HashMap<(NodeId, usize), usize>,
    mask_slot: HashMap<(NodeId, usize), usize>,
}

fn layout(g: &Graph, ch: &ChannelSettings, image_layer: &dyn Fn(&str) -> i32) -> Layout {
    let mut l = Layout {
        params: Vec::new(),
        node_param: HashMap::new(),
        slot: HashMap::new(),
        mask_slot: HashMap::new(),
    };
    for c in Channel::ALL {
        let d = ch.defaults[c.index()];
        l.params.push([d[0], d[1], d[2], 0.0]);
    }
    for n in g.nodes.values() {
        for p in &n.def().params {
            let off = l.params.len();
            l.node_param.insert((n.id, p.name), off);
            let v = n.param(p.name);
            match p.kind {
                ParamKind::Float { .. } | ParamKind::Int { .. } | ParamKind::Bool => {
                    l.params.push([v.as_f32(), 0.0, 0.0, 0.0])
                }
                ParamKind::Color | ParamKind::Vec3 => {
                    let c = v.as_vec3();
                    l.params.push([c[0], c[1], c[2], 0.0]);
                }
                ParamKind::Enum(options) => {
                    let idx = options.iter().position(|o| *o == v.as_str()).unwrap_or(0) as f32;
                    // Gradient axis: also store the axis vector.
                    if n.ty == "gradient" && p.name == "axis" {
                        let axis = match idx as usize {
                            0 => [1.0, 0.0, 0.0],
                            1 => [0.0, 1.0, 0.0],
                            2 => [0.0, 0.0, 1.0],
                            _ => n.param("custom_axis").as_vec3(),
                        };
                        l.params.push([axis[0], axis[1], axis[2], idx]);
                    } else {
                        l.params.push([idx, 0.0, 0.0, 0.0]);
                    }
                }
                ParamKind::Image => l.params.push([image_layer(v.as_str()) as f32, 0.0, 0.0, 0.0]),
                ParamKind::Stops => {
                    let stops = match v {
                        Value::Stops(s) => s,
                        _ => vec![],
                    };
                    l.params.push([stops.len().min(MAX_STOPS) as f32, 0.0, 0.0, 0.0]);
                    for k in 0..MAX_STOPS {
                        let (pos, c) = stops.get(k).copied().unwrap_or((1.0, [0.0; 3]));
                        l.params.push([c[0], c[1], c[2], pos]);
                    }
                }
                ParamKind::Colors => {
                    let colors = match v {
                        Value::Colors(c) => c,
                        _ => vec![],
                    };
                    l.params.push([colors.len().min(MAX_ID_COLORS) as f32, 0.0, 0.0, 0.0]);
                    for k in 0..MAX_ID_COLORS {
                        let c = colors.get(k).copied().unwrap_or([-1.0; 3]);
                        l.params.push([c[0], c[1], c[2], 0.0]);
                    }
                }
            }
        }
        for (i, s) in n.slots.iter().enumerate() {
            l.slot.insert((n.id, i), l.params.len());
            l.params.push([s.opacity, 0.0, 0.0, 0.0]);
            for cb in &s.channels {
                l.params.push([cb.enabled as u8 as f32, cb.mode.code() as f32, cb.opacity, 0.0]);
            }
        }
        for (i, m) in n.mask_slots.iter().enumerate() {
            l.mask_slot.insert((n.id, i), l.params.len());
            l.params.push([m.opacity, m.mode.code() as f32, 0.0, 0.0]);
            if let MaskSource::Levels { in_black, in_white, gamma, out_black, out_white } = m.source {
                l.params.push([in_black, in_white, gamma, out_black]);
                l.params.push([out_white, 0.0, 0.0, 0.0]);
            }
        }
    }
    l
}

#[derive(Default)]
struct Func {
    lines: Vec<String>,
    emitted: HashSet<NodeId>,
}

/// A WGSL function with `header` (its signature) and `lines` as its body.
fn function(header: &str, lines: &[String]) -> String {
    let mut code = format!("{header} {{\n");
    for line in lines {
        let _ = writeln!(code, "    {line}");
    }
    code.push_str("}\n");
    code
}

struct Ctx<'a> {
    g: &'a Graph,
    ch: &'a ChannelSettings,
    l: Layout,
    funcs: Vec<String>,
    func_done: HashSet<NodeId>,
    errors: Vec<String>,
    paint_slot: &'a dyn Fn(NodeId) -> Option<u32>,
    cache: &'a dyn Fn(NodeId) -> Option<CacheRef>,
    /// Node evaluated for its own cache: never read from the cache.
    exclude: Option<NodeId>,
}

/// Where a cached node's result lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheRef {
    /// Float/Color node: one layer per tile in the cache pool, rgb = color, a = float.
    Value(u32),
    /// Material node: a slot in the paint pools, in paint-plane layout.
    Material(u32),
}

/// Everything compilation needs to know about the project besides the graph.
pub struct Env<'a> {
    pub image_layer: &'a dyn Fn(&str) -> i32,
    pub paint_slot: &'a dyn Fn(NodeId) -> Option<u32>,
    pub cache: &'a dyn Fn(NodeId) -> Option<CacheRef>,
    /// Node Editor preview: show this output instead of the Material Output (viewport only).
    pub preview: Option<&'a Link>,
}

/// What a cache evaluation shader computes.
#[derive(Clone, Debug)]
pub enum Target {
    /// Every output of a node.
    Node(NodeId),
    /// The value arriving at a node input (filter inputs).
    Link(Link, SocketType),
}

/// Compiles the material. Paint nodes read their pool slot; cached nodes read their cache.
pub fn compile(g: &Graph, ch: &ChannelSettings, env: &Env) -> Compiled {
    let mut cx = Ctx::new(g, ch, env, None);
    let mut f = Func::default();
    let body = match env.preview.filter(|l| g.get(l.node).is_some()) {
        Some(link) => match g.output_type(link) {
            Some(SocketType::Material) => cx.input_expr(&mut f, link, SocketType::Material),
            _ => format!("mat_preview({})", cx.input_expr(&mut f, link, SocketType::Color)),
        },
        None => match g.output_node().and_then(|o| g.node(o).inputs.get("in").cloned()) {
            Some(link) => cx.input_expr(&mut f, &link, SocketType::Material),
            None => "mat_defaults()".to_string(),
        },
    };
    f.lines.push(format!("return apply_baked_normal(s, mat_to_material({body}));"));
    let code = cx.finish("fn eval_material(s: Surface) -> Material", &f);
    Compiled { code, params: cx.l.params, errors: cx.errors }
}

/// Compiles `fn eval_target(s) -> CacheOut` computing `target`, for filling a cache.
/// The target node itself is evaluated, not read from its cache.
pub fn compile_target(g: &Graph, ch: &ChannelSettings, env: &Env, target: &Target) -> String {
    let exclude = match target {
        Target::Node(n) => Some(*n),
        Target::Link(..) => None,
    };
    let mut cx = Ctx::new(g, ch, env, exclude);
    let mut f = Func::default();
    let (value, mat) = match target {
        Target::Node(n) => {
            cx.emit(&mut f, *n);
            let node = g.node(*n);
            let outs = node.def().outputs;
            if outs.iter().any(|(_, t)| *t == SocketType::Material) {
                ("vec4f(0.0)".to_string(), var(*n, outs[0].0))
            } else {
                let color = outs.iter().find(|(_, t)| *t == SocketType::Color).map(|(o, _)| var(*n, o));
                let float = outs.iter().find(|(_, t)| *t == SocketType::Float).map(|(o, _)| var(*n, o));
                let c = color.clone().or_else(|| float.clone().map(|f| format!("vec3f({f})"))).unwrap_or("vec3f(0.0)".into());
                let a = float.or_else(|| color.map(|c| format!("luminance({c})"))).unwrap_or("0.0".into());
                (format!("vec4f({c}, {a})"), "mat_defaults()".to_string())
            }
        }
        Target::Link(link, t) => {
            let e = cx.input_expr(&mut f, link, *t);
            match t {
                SocketType::Float => (format!("vec4f(vec3f({e}), {e})"), "mat_defaults()".into()),
                SocketType::Color | SocketType::Vector => (format!("vec4f({e}, luminance({e}))"), "mat_defaults()".into()),
                SocketType::Material => ("vec4f(0.0)".into(), e),
            }
        }
    };
    f.lines.push(format!("return CacheOut({value}, {mat});"));
    cx.finish("fn eval_target(s: Surface) -> CacheOut", &f)
}

/// Whether a node's outputs fit a cache (at most one Color and one Float, or one Material).
pub fn cacheable(n: &Node) -> bool {
    let outs = n.def().outputs;
    let count = |t| outs.iter().filter(|(_, x)| *x == t).count();
    !outs.is_empty()
        && count(SocketType::Vector) == 0
        && !matches!(n.ty.as_str(), "material_output" | "paint" | "paint_mask")
        && ((count(SocketType::Material) == 1 && outs.len() == 1)
            || (count(SocketType::Color) <= 1 && count(SocketType::Float) <= 1 && count(SocketType::Material) == 0))
}

fn var(id: NodeId, output: &str) -> String {
    format!("n{id}_{output}")
}

fn convert(expr: String, from: SocketType, to: SocketType) -> String {
    use SocketType::*;
    match (from, to) {
        (a, b) if a == b => expr,
        (Float, Color | Vector) => format!("vec3f({expr})"),
        (Color, Float) => format!("luminance({expr})"),
        (Vector, Float) => format!("dot({expr}, vec3f(1.0 / 3.0))"),
        (Color, Vector) | (Vector, Color) => expr,
        // Material conversions are not implicit; fall back to defaults.
        (_, Material) => "mat_defaults()".into(),
        (Material, Float) => "0.0".into(),
        (Material, Color | Vector) => "vec3f(0.0)".into(),
        _ => expr,
    }
}

fn default_for(t: SocketType) -> &'static str {
    match t {
        SocketType::Float => "0.0",
        SocketType::Color | SocketType::Vector => "vec3f(0.0)",
        SocketType::Material => "mat_defaults()",
    }
}

impl<'a> Ctx<'a> {
    fn new(g: &'a Graph, ch: &'a ChannelSettings, env: &Env<'a>, exclude: Option<NodeId>) -> Self {
        Ctx {
            g,
            ch,
            l: layout(g, ch, env.image_layer),
            funcs: Vec::new(),
            func_done: HashSet::new(),
            errors: Vec::new(),
            paint_slot: env.paint_slot,
            cache: env.cache,
            exclude,
        }
    }

    /// The generated stack functions followed by the entry point `header` with body `f`.
    fn finish(&self, header: &str, f: &Func) -> String {
        let mut code: String = self.funcs.iter().map(|func| format!("{func}\n")).collect();
        code.push_str(&function(header, &f.lines));
        code
    }

    fn p(&self, n: &Node, name: &str) -> String {
        let def = n.def().params.iter().find(|p| p.name == name);
        let Some(def) = def else { return "0.0".into() };
        let off = self.l.node_param[&(n.id, def.name)];
        match def.kind {
            ParamKind::Color | ParamKind::Vec3 => format!("params[{off}].xyz"),
            _ => format!("params[{off}].x"),
        }
    }

    fn off(&self, n: &Node, name: &str) -> usize {
        let def = n.def().params.iter().find(|p| p.name == name).expect("param exists");
        self.l.node_param[&(n.id, def.name)]
    }

    fn input_expr(&mut self, f: &mut Func, link: &Link, want: SocketType) -> String {
        let Some(from_t) = self.g.output_type(link) else {
            return default_for(want).into();
        };
        self.emit(f, link.node);
        convert(var(link.node, &link.output), from_t, want)
    }

    fn input(&mut self, f: &mut Func, n: &Node, name: &str, want: SocketType, default: &str) -> String {
        match n.inputs.get(name).cloned() {
            Some(link) => self.input_expr(f, &link, want),
            None => default.to_string(),
        }
    }

    /// Emits `let` bindings for every output of `id` into `f`.
    fn emit(&mut self, f: &mut Func, id: NodeId) {
        if f.emitted.contains(&id) {
            return;
        }
        f.emitted.insert(id);
        let g = self.g;
        let Some(n) = g.get(id) else { return };
        if self.exclude != Some(id) {
            if let Some(c) = (self.cache)(id) {
                match c {
                    CacheRef::Material(slot) => {
                        f.lines.push(format!("let {} = paint_sample({slot}u, s);", var(id, n.def().outputs[0].0)));
                    }
                    CacheRef::Value(slot) => {
                        let cv = format!("n{id}_cached");
                        f.lines.push(format!("let {cv} = cache_sample({slot}u, s);"));
                        for (o, t) in n.def().outputs {
                            let e = if *t == SocketType::Color { format!("{cv}.rgb") } else { format!("{cv}.a") };
                            f.lines.push(format!("let {} = {e};", var(id, o)));
                        }
                    }
                }
                return;
            }
        }
        let o = |out: &str| var(id, out);
        let mut lines: Vec<String> = Vec::new();
        match n.ty.as_str() {
            "paint" => match (self.paint_slot)(id) {
                Some(slot) => lines.push(format!("let {} = paint_sample({slot}u, s);", o("out"))),
                None => lines.push(format!("let {} = mat_uncovered(mat_defaults());", o("out"))),
            },
            "paint_mask" => match (self.paint_slot)(id) {
                Some(slot) => lines.push(format!("let {} = paint_mask_sample({slot}u, s).x;", o("out"))),
                None => lines.push(format!("let {} = 0.0;", o("out"))),
            },
            "value" => lines.push(format!("let {} = {};", o("value"), self.p(n, "value"))),
            "color" => lines.push(format!("let {} = {};", o("color"), self.p(n, "color"))),
            "vector" => lines.push(format!("let {} = {};", o("vector"), self.p(n, "vector"))),
            "uv" => lines.push(format!("let {} = vec3f(s.tile_uv, 0.0);", o("uv"))),
            "geometry" => {
                lines.push(format!("let {} = s.pos;", o("position")));
                lines.push(format!("let {} = s.pos_norm;", o("generated")));
                lines.push(format!("let {} = s.normal;", o("normal")));
            }
            "image" => {
                let uv = n.inputs.contains_key("vector").then(|| self.input(f, n, "vector", SocketType::Vector, "vec3f(0.0)"));
                let sample = self.image_sample(n, "image", uv);
                lines.push(format!("let {}_raw = {sample};", o("img")));
                lines.push(format!(
                    "let {} = select({img}_raw.rgb, srgb_to_linear({img}_raw.rgb), {} > 0.5);",
                    o("color"),
                    self.p(n, "srgb"),
                    img = o("img")
                ));
                lines.push(format!("let {} = {}_raw.a;", o("alpha"), o("img")));
            }
            "mesh_map" => {
                let map = n.param("map");
                let tc = "tile_tex(s.tile_uv)";
                let (fl, co) = match map.as_str() {
                    "Curvature" => (format!("mesh_curvature({tc}, s.layer)"), None),
                    "Thickness" => (format!("mesh_thickness({tc}, s.layer)"), None),
                    "World Normal" => {
                        (String::new(), Some(format!("mesh_world_normal({tc}, s.layer)")))
                    }
                    "Position" => (String::new(), Some(format!("mesh_position({tc}, s.layer)"))),
                    "ID" => (String::new(), Some(format!("mesh_id({tc}, s.layer)"))),
                    "Normal" => (String::new(), Some(format!("mesh_normal_map({tc}, s.layer).xyz"))),
                    "Height" => (format!("mesh_height({tc}, s.layer)"), None),
                    _ => (format!("mesh_ao({tc}, s.layer)"), None),
                };
                match co {
                    Some(c) => {
                        lines.push(format!("let {} = {c};", o("color")));
                        lines.push(format!("let {} = luminance({});", o("float"), o("color")));
                    }
                    None => {
                        lines.push(format!("let {} = {fl};", o("float")));
                        lines.push(format!("let {} = vec3f({});", o("color"), o("float")));
                    }
                }
            }
            "noise" => {
                let kind = graph::NOISE_TYPES.iter().position(|t| *t == n.param("type").as_str()).unwrap_or(0);
                let p = self.input(f, n, "vector", SocketType::Vector, "s.pos_norm");
                lines.push(format!(
                    "let {} = fbm({p} * {}, {kind}u, {}, {}, {}, {});",
                    o("out"),
                    self.p(n, "scale"),
                    self.p(n, "octaves"),
                    self.p(n, "lacunarity"),
                    self.p(n, "gain"),
                    self.p(n, "seed")
                ));
            }
            "cellular" => {
                let manhattan = n.param("metric").as_str() == "Manhattan";
                let p = self.input(f, n, "vector", SocketType::Vector, "s.pos_norm");
                lines.push(format!(
                    "let {}_c = cellular({p} * {}, {}, {manhattan}, {});",
                    o("cell"),
                    self.p(n, "scale"),
                    self.p(n, "jitter"),
                    self.p(n, "seed")
                ));
                let c = o("cell");
                let expr = match n.param("output").as_str() {
                    "F2" => format!("clamp({c}_c.y * 0.75, 0.0, 1.0)"),
                    "F2-F1" => format!("clamp(({c}_c.y - {c}_c.x) * 1.5, 0.0, 1.0)"),
                    _ => format!("clamp({c}_c.x, 0.0, 1.0)"),
                };
                lines.push(format!("let {} = {expr};", o("out")));
            }
            "gradient" => {
                let axis_off = self.off(n, "axis");
                let space_off = self.off(n, "space");
                let p = match n.inputs.contains_key("vector") {
                    true => self.input(f, n, "vector", SocketType::Vector, "vec3f(0.0)"),
                    false => format!("select(s.pos_norm, s.pos, params[{space_off}].x > 0.5)"),
                };
                lines.push(format!("let {}_p = {p};", o("g")));
                lines.push(format!(
                    "let {} = clamp((dot({g}_p, normalize(params[{axis_off}].xyz)) - {start}) / max({end} - {start}, 1e-5), 0.0, 1.0);",
                    o("out"),
                    g = o("g"),
                    start = self.p(n, "start"),
                    end = self.p(n, "end"),
                ));
            }
            "id_select" => {
                let off = self.off(n, "colors");
                let count = match n.param("colors") {
                    Value::Colors(c) => c.len().min(MAX_ID_COLORS),
                    _ => 0,
                };
                let mut terms = vec!["0.0".to_string()];
                for k in 0..count {
                    terms.push(format!("id_match(s, params[{}].xyz, {})", off + 1 + k, self.p(n, "tolerance")));
                }
                let mut expr = terms[0].clone();
                for t in &terms[1..] {
                    expr = format!("max({expr}, {t})");
                }
                lines.push(format!("let {} = {expr};", o("out")));
            }
            "edge_wear" => lines.push(format!(
                "let {} = gen_edge_wear(s, {}, {}, {}, {}, {});",
                o("out"),
                self.p(n, "amount"),
                self.p(n, "sharpness"),
                self.p(n, "noise_scale"),
                self.p(n, "noise_amount"),
                self.p(n, "seed")
            )),
            "dirt" => lines.push(format!(
                "let {} = gen_dirt(s, {}, {}, {}, {});",
                o("out"),
                self.p(n, "amount"),
                self.p(n, "contrast"),
                self.p(n, "noise_scale"),
                self.p(n, "seed")
            )),
            "curvature_mask" => lines.push(format!(
                "let {} = gen_curvature_mask(s, {} > 0.5, {}, {});",
                o("out"),
                self.p(n, "select"),
                self.p(n, "threshold"),
                self.p(n, "softness")
            )),
            "position_gradient" => lines.push(format!(
                "let {} = gen_position_gradient(s, u32({}), {}, {});",
                o("out"),
                self.p(n, "axis"),
                self.p(n, "falloff"),
                self.p(n, "noise_amount")
            )),
            "levels" => {
                let i = self.input(f, n, "in", SocketType::Color, "vec3f(0.0)");
                lines.push(format!(
                    "let {} = levels3({i}, {}, {}, {}, {}, {});",
                    o("out"),
                    self.p(n, "in_black"),
                    self.p(n, "in_white"),
                    self.p(n, "gamma"),
                    self.p(n, "out_black"),
                    self.p(n, "out_white")
                ));
                lines.push(format!("let {} = {}.x;", o("float"), o("out")));
            }
            "ramp" => {
                let i = self.input(f, n, "in", SocketType::Float, "0.0");
                let off = self.off(n, "stops");
                let count = match n.param("stops") {
                    Value::Stops(s) => s.len().clamp(1, MAX_STOPS),
                    _ => 1,
                };
                let r = o("out");
                lines.push(format!("let {r}_t = {i};"));
                lines.push(format!("var {r}_c = params[{}].xyz;", off + 1));
                for k in 1..count {
                    let (a, b) = (off + k, off + 1 + k);
                    lines.push(format!(
                        "{r}_c = select({r}_c, mix(params[{a}].xyz, params[{b}].xyz, clamp(({r}_t - params[{a}].w) / max(params[{b}].w - params[{a}].w, 1e-5), 0.0, 1.0)), {r}_t > params[{a}].w);"
                    ));
                }
                lines.push(format!("let {r} = {r}_c;"));
            }
            "invert" => {
                let i = self.input(f, n, "in", SocketType::Color, "vec3f(0.0)");
                lines.push(format!("let {} = 1.0 - {i};", o("out")));
                lines.push(format!("let {} = {}.x;", o("float"), o("out")));
            }
            "clamp" => {
                let i = self.input(f, n, "in", SocketType::Color, "vec3f(0.0)");
                lines.push(format!(
                    "let {} = clamp({i}, vec3f({}), vec3f({}));",
                    o("out"),
                    self.p(n, "min"),
                    self.p(n, "max")
                ));
                lines.push(format!("let {} = {}.x;", o("float"), o("out")));
            }
            "hsv_adjust" => {
                let i = self.input(f, n, "in", SocketType::Color, "vec3f(0.0)");
                let h = o("hsv");
                lines.push(format!("let {h} = rgb_to_hsv(max({i}, vec3f(0.0)));"));
                lines.push(format!(
                    "let {} = hsv_to_rgb(vec3f(fract({h}.x + {}), clamp({h}.y * {}, 0.0, 1.0), {h}.z * {}));",
                    o("out"),
                    self.p(n, "hue"),
                    self.p(n, "saturation"),
                    self.p(n, "value")
                ));
            }
            "math" => {
                let a = self.input(f, n, "a", SocketType::Float, "0.0");
                let bdef = self.p(n, "b_value");
                let b = self.input(f, n, "b", SocketType::Float, &bdef);
                let expr = match n.param("op").as_str() {
                    "Add" => format!("{a} + {b}"),
                    "Subtract" => format!("{a} - {b}"),
                    "Divide" => format!("{a} / select({b}, 1e-5, abs({b}) < 1e-5)"),
                    "Power" => format!("pow(max({a}, 0.0), {b})"),
                    "Min" => format!("min({a}, {b})"),
                    "Max" => format!("max({a}, {b})"),
                    "Abs" => format!("abs({a})"),
                    "Smoothstep" => format!("smoothstep(0.0, max({b}, 1e-5), {a})"),
                    _ => format!("{a} * {b}"),
                };
                lines.push(format!("let {} = {expr};", o("out")));
            }
            "uv_transform" => {
                let v = self.input(f, n, "vector", SocketType::Vector, "vec3f(s.tile_uv, 0.0)");
                let r = o("vector");
                lines.push(format!("let {r}_in = {v};"));
                lines.push(format!(
                    "let {r} = vec3f(rot2({r}_in.xy - 0.5, {}) * vec2f({}, {}) + 0.5 + vec2f({}, {}), {r}_in.z);",
                    self.p(n, "rotation"),
                    self.p(n, "tile_u"),
                    self.p(n, "tile_v"),
                    self.p(n, "offset_u"),
                    self.p(n, "offset_v")
                ));
            }
            "mapping" => {
                let v = self.input(f, n, "vector", SocketType::Vector, "vec3f(s.tile_uv, 0.0)");
                lines.push(format!(
                    "let {} = euler_xyz({}) * ({v} * {}) + {};",
                    o("vector"),
                    self.p(n, "rotation"),
                    self.p(n, "scale"),
                    self.p(n, "location")
                ));
            }
            "vector_math" => {
                let a = self.input(f, n, "a", SocketType::Vector, "vec3f(0.0)");
                let bdef = self.p(n, "b_value");
                let b = self.input(f, n, "b", SocketType::Vector, &bdef);
                let r = o("vm");
                lines.push(format!("let {r}_a = {a};"));
                lines.push(format!("let {r}_b = {b};"));
                let (a, b) = (format!("{r}_a"), format!("{r}_b"));
                let zero = "vec3f(0.0)".to_string();
                let (vector, value) = match n.param("op").as_str() {
                    "Subtract" => (format!("{a} - {b}"), "0.0".into()),
                    "Multiply" => (format!("{a} * {b}"), "0.0".into()),
                    "Divide" => (format!("select({a} / {b}, vec3f(0.0), {b} == vec3f(0.0))"), "0.0".into()),
                    "Scale" => (format!("{a} * {}", self.p(n, "scale")), "0.0".into()),
                    "Dot Product" => (zero, format!("dot({a}, {b})")),
                    "Cross Product" => (format!("cross({a}, {b})"), "0.0".into()),
                    "Length" => (zero, format!("length({a})")),
                    "Distance" => (zero, format!("distance({a}, {b})")),
                    "Normalize" => (format!("select(normalize({a}), vec3f(0.0), dot({a}, {a}) == 0.0)"), "0.0".into()),
                    "Absolute" => (format!("abs({a})"), "0.0".into()),
                    "Floor" => (format!("floor({a})"), "0.0".into()),
                    "Fraction" => (format!("fract({a})"), "0.0".into()),
                    "Minimum" => (format!("min({a}, {b})"), "0.0".into()),
                    "Maximum" => (format!("max({a}, {b})"), "0.0".into()),
                    _ => (format!("{a} + {b}"), "0.0".into()),
                };
                lines.push(format!("let {} = {vector};", o("vector")));
                lines.push(format!("let {} = {value};", o("value")));
            }
            "combine_xyz" => {
                let [x, y, z] = ["x", "y", "z"].map(|c| {
                    let d = self.p(n, c);
                    self.input(f, n, c, SocketType::Float, &d)
                });
                lines.push(format!("let {} = vec3f({x}, {y}, {z});", o("vector")));
            }
            "separate_xyz" => {
                let v = self.input(f, n, "vector", SocketType::Vector, "vec3f(0.0)");
                lines.push(format!("let {}_in = {v};", o("xyz")));
                for c in ["x", "y", "z"] {
                    lines.push(format!("let {} = {}_in.{c};", o(c), o("xyz")));
                }
            }
            "merge" => {
                let a = self.input(f, n, "a", SocketType::Color, "vec3f(0.0)");
                let b = self.input(f, n, "b", SocketType::Color, "vec3f(0.0)");
                let m = self.input(f, n, "mask", SocketType::Float, "1.0");
                lines.push(format!(
                    "let {} = mix({a}, blend3({a}, {b}, u32({})), clamp({m}, 0.0, 1.0) * {});",
                    o("out"),
                    self.p(n, "mode"),
                    self.p(n, "opacity")
                ));
                lines.push(format!("let {} = {}.x;", o("float"), o("out")));
            }
            "material_combine" => {
                let r = o("out");
                lines.push(format!("var {r}_m = mat_uncovered(mat_defaults());"));
                for c in Channel::ALL {
                    let t = if c.components() == 3 { SocketType::Color } else { SocketType::Float };
                    if let Some(link) = n.inputs.get(c.field()).cloned() {
                        let e = self.input_expr(f, &link, t);
                        let v = match (c, t) {
                            (Channel::Normal, _) => format!("normalize({e} * 2.0 - 1.0)"),
                            (_, SocketType::Float) => format!("vec3f({e})"),
                            _ => e,
                        };
                        lines.push(format!("{r}_m.v[{}] = {v};", c.index()));
                        lines.push(format!("{r}_m.a[{}] = 1.0;", c.index()));
                    }
                }
                lines.push(format!("let {r} = {r}_m;"));
            }
            "material_split" => {
                let i = self.input(f, n, "in", SocketType::Material, "mat_defaults()");
                let m = o("m");
                lines.push(format!("let {m} = {i};"));
                for c in Channel::ALL {
                    let e = match c {
                        Channel::Normal => format!("{m}.v[4] * 0.5 + 0.5"),
                        _ if c.components() == 3 => format!("{m}.v[{}]", c.index()),
                        _ => format!("{m}.v[{}].x", c.index()),
                    };
                    lines.push(format!("let {} = {e};", o(c.field())));
                }
            }
            "fill" => self.emit_fill(n, &mut lines),
            "stack" => {
                self.stack_function(id);
                lines.push(format!("let {} = stack_{id}(s, mat_uncovered(mat_defaults()));", o("out")));
            }
            "mask_stack" => {
                self.mask_function(id);
                lines.push(format!("let {} = mstack_{id}(s);", o("out")));
            }
            // Filters are evaluated through the cache (see cache.rs); without a cached
            // result they pass their input through.
            "blur" => {
                let i = self.input(f, n, "in", SocketType::Color, "vec3f(0.0)");
                lines.push(format!("let {} = {i};", o("out")));
                lines.push(format!("let {} = {}.x;", o("float"), o("out")));
            }
            "dilate" => {
                let i = self.input(f, n, "in", SocketType::Float, "0.0");
                lines.push(format!("let {} = {i};", o("out")));
            }
            "height_to_normal" => {
                lines.push(format!("let {} = vec3f(0.5, 0.5, 1.0);", o("out")));
            }
            "material_output" => {}
            other => self.errors.push(format!("Unknown node type {other}")),
        }
        f.lines.extend(lines);
    }

    /// An Image node's (or Fill channel's) sample; `uv` is the Image node's Vector input.
    fn image_sample(&self, n: &Node, image_param: &str, uv: Option<String>) -> String {
        let layer = format!("i32({})", self.p(n, image_param));
        let (scale, rot, offset) = (self.p(n, "scale"), self.p(n, "rotation"), self.p(n, "offset"));
        if let Some(uv) = uv {
            format!("image_vec({layer}, ({uv}).xy, {scale}, {rot}, {offset})")
        } else if n.param("projection").as_str() == "Triplanar" {
            format!("image_triplanar({layer}, s, {scale}, {rot}, {offset}, {})", self.p(n, "triplanar_blend"))
        } else {
            format!("image_uv({layer}, s, {scale}, {rot}, {offset})")
        }
    }

    fn emit_fill(&self, n: &Node, lines: &mut Vec<String>) {
        let r = var(n.id, "out");
        lines.push(format!("var {r}_m = mat_uncovered(mat_defaults());"));
        for c in Channel::ALL {
            if !self.ch.is_enabled(c) {
                continue;
            }
            let (on, val, src, img) = graph::fill_param_names(c);
            let value = if n.param(src).as_str() == "Image" {
                let sample = self.image_sample(n, img, None);
                match c {
                    Channel::BaseColor | Channel::Emissive => format!("srgb_to_linear(({sample}).rgb)"),
                    Channel::Normal => format!("normalize(({sample}).rgb * 2.0 - 1.0)"),
                    Channel::Height => format!("vec3f(({sample}).r * 2.0 - 1.0)"),
                    _ => format!("vec3f(({sample}).r)"),
                }
            } else {
                match c {
                    Channel::Normal => format!("normalize({} * 2.0 - 1.0)", self.p(n, val)),
                    _ if c.components() == 3 => self.p(n, val),
                    _ => format!("vec3f({})", self.p(n, val)),
                }
            };
            lines.push(format!("{r}_m.v[{}] = {value};", c.index()));
            lines.push(format!("{r}_m.a[{}] = {};", c.index(), self.p(n, on)));
        }
        lines.push(format!("let {r} = {r}_m;"));
    }

    fn stack_function(&mut self, id: NodeId) {
        if !self.func_done.insert(id) {
            return;
        }
        let g = self.g;
        let n = g.node(id);
        let mut f = Func::default();
        f.lines.push("var acc = below;".into());
        for (i, slot) in n.slots.iter().enumerate() {
            let Some(content) = slot.content else { continue };
            if !slot.visible || g.get(content).is_none() {
                continue;
            }
            let off = self.l.slot[&(id, i)];
            let is_group = g.node(content).ty == "stack";
            let lay = format!("lay{i}");
            if is_group {
                self.stack_function(content);
                f.lines.push(format!("let {lay} = stack_{content}(s, mat_uncovered(acc));"));
            } else {
                let link = Link { node: content, output: "out".into() };
                let e = self.input_expr(&mut f, &link, SocketType::Material);
                f.lines.push(format!("let {lay} = {e};"));
            }
            let mask = match slot.mask {
                Some(m) if g.get(m).is_some() => {
                    self.mask_function(m);
                    format!("mstack_{m}(s)")
                }
                _ => "1.0".into(),
            };
            f.lines.push(format!("let mk{i} = {mask} * params[{off}].x;"));
            for c in Channel::ALL {
                if !self.ch.is_enabled(c) {
                    continue;
                }
                let ci = c.index();
                let cp = format!("params[{}]", off + 1 + ci);
                let t = format!("mk{i} * {cp}.x * {cp}.z");
                if is_group {
                    // The group composited onto a copy of `acc`; Normal fades to that result.
                    f.lines.push(format!(
                        "composite(&acc, {ci}u, {lay}.v[{ci}], select({t} * {lay}.a[{ci}], {t}, u32({cp}.y) == 0u || u32({cp}.y) == {combine}u), select(u32({cp}.y), 0u, u32({cp}.y) == {combine}u));",
                        combine = BlendMode::Combine.code()
                    ));
                } else {
                    f.lines.push(format!(
                        "composite(&acc, {ci}u, {lay}.v[{ci}], {lay}.a[{ci}] * {t}, u32({cp}.y));"
                    ));
                }
            }
        }
        f.lines.push("return acc;".into());
        self.funcs.push(function(&format!("fn stack_{id}(s: Surface, below: Mat) -> Mat"), &f.lines));
    }

    fn mask_function(&mut self, id: NodeId) {
        if !self.func_done.insert(id) {
            return;
        }
        let g = self.g;
        let n = g.node(id);
        let mut f = Func::default();
        f.lines.push("var m = 0.0;".into());
        for (i, slot) in n.mask_slots.iter().enumerate() {
            if !slot.visible {
                continue;
            }
            let off = self.l.mask_slot[&(id, i)];
            match &slot.source {
                MaskSource::Node(src) => {
                    let Some(src_node) = g.get(*src) else { continue };
                    if src_node.ty == "paint_mask" {
                        match (self.paint_slot)(*src) {
                            Some(ps) => {
                                f.lines.push(format!("let pm{i} = paint_mask_sample({ps}u, s);"));
                                f.lines.push(format!("let v{i} = pm{i}.x / max(pm{i}.y, 1e-5);"));
                                f.lines.push(format!("let a{i} = pm{i}.y;"));
                            }
                            None => continue,
                        }
                    } else {
                        let out = if src_node.def().outputs.iter().any(|(o, _)| *o == "out") {
                            "out"
                        } else {
                            src_node.def().outputs.first().map(|(o, _)| *o).unwrap_or("out")
                        };
                        let link = Link { node: *src, output: out.into() };
                        let e = self.input_expr(&mut f, &link, SocketType::Float);
                        f.lines.push(format!("let v{i} = {e};"));
                        f.lines.push(format!("let a{i} = 1.0;"));
                    }
                    f.lines.push(format!(
                        "m = mix(m, blendf(m, v{i}, u32(params[{off}].y)), a{i} * params[{off}].x);"
                    ));
                }
                MaskSource::Levels { .. } => {
                    f.lines.push(format!(
                        "m = mix(m, levels3(vec3f(m), params[{a}].x, params[{a}].y, params[{a}].z, params[{a}].w, params[{b}].x).x, params[{off}].x);",
                        a = off + 1,
                        b = off + 2
                    ));
                }
            }
            f.lines.push("m = clamp(m, 0.0, 1.0);".into());
        }
        f.lines.push("return m;".into());
        self.funcs.push(function(&format!("fn mstack_{id}(s: Surface) -> f32"), &f.lines));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(g: &Graph, preview: &Link) -> String {
        let env = Env { image_layer: &|_| 0, paint_slot: &|_| None, cache: &|_| None, preview: Some(preview) };
        let c = compile(g, &ChannelSettings::default(), &env);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        c.code
    }

    #[test]
    fn image_reads_its_vector_input_as_uv() {
        let mut g = Graph::new();
        let img = g.add("image");
        let link = Link { node: img, output: "color".into() };
        assert!(code(&g, &link).contains("image_uv("));
        let uv = g.add("uv");
        g.connect(uv, "uv", img, "vector");
        let c = code(&g, &link);
        assert!(c.contains(&format!("let n{uv}_uv = vec3f(s.tile_uv, 0.0);")));
        assert!(c.contains(&format!("image_vec(i32(params[")) && c.contains(&format!("(n{uv}_uv).xy")), "{c}");
    }

    #[test]
    fn vector_nodes_are_not_cacheable() {
        let mut g = Graph::new();
        for ty in ["uv", "geometry", "vector", "uv_transform", "mapping", "vector_math", "combine_xyz"] {
            let n = g.add(ty);
            assert!(!cacheable(g.node(n)), "{ty}");
        }
        let n = g.add("separate_xyz");
        assert!(!cacheable(g.node(n)), "three Float outputs");
    }
}
