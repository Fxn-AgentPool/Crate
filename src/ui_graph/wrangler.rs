//! Node Wrangler-style graph operations (after Blender's add-on): merge, lazy connect, swap,
//! dissolve, duplicate, align, reset, copy settings. Each works on the graph only; the editor
//! wraps them in an undo step and recompiles.

use std::collections::BTreeMap;

use super::{HEADER, NEW_LAYER, NODE_W, ROW, SINK, SocketRef, compatible, connect, inputs, links_into, outputs};
use crate::graph::{self, Graph, NodeId, SocketType, Value};
use crate::ops::LayerRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeOp {
    Mix,
    Add,
    Subtract,
    Multiply,
    Divide,
    Min,
    Max,
}

impl MergeOp {
    pub const ALL: [MergeOp; 7] =
        [MergeOp::Mix, MergeOp::Add, MergeOp::Subtract, MergeOp::Multiply, MergeOp::Divide, MergeOp::Min, MergeOp::Max];

    pub fn label(self) -> &'static str {
        match self {
            MergeOp::Mix => "Mix",
            MergeOp::Add => "Add",
            MergeOp::Subtract => "Subtract",
            MergeOp::Multiply => "Multiply",
            MergeOp::Divide => "Divide",
            MergeOp::Min => "Minimum",
            MergeOp::Max => "Maximum",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            MergeOp::Mix => "Ctrl+0",
            MergeOp::Add => "Ctrl++",
            MergeOp::Subtract => "Ctrl+-",
            MergeOp::Multiply => "Ctrl+8",
            MergeOp::Divide => "Ctrl+/",
            MergeOp::Min => "Ctrl+,",
            MergeOp::Max => "Ctrl+.",
        }
    }

    /// The Math node's `op` for float merges (Mix has none).
    fn math(self) -> Option<&'static str> {
        match self {
            MergeOp::Mix => None,
            MergeOp::Add => Some("Add"),
            MergeOp::Subtract => Some("Subtract"),
            MergeOp::Multiply => Some("Multiply"),
            MergeOp::Divide => Some("Divide"),
            MergeOp::Min => Some("Min"),
            MergeOp::Max => Some("Max"),
        }
    }

    /// The Merge node's blend `mode` for color merges (Divide has none).
    fn blend(self) -> Option<&'static str> {
        match self {
            MergeOp::Mix => Some("Normal"),
            MergeOp::Add => Some("Add"),
            MergeOp::Subtract => Some("Subtract"),
            MergeOp::Multiply => Some("Multiply"),
            MergeOp::Divide => None,
            MergeOp::Min => Some("Darken"),
            MergeOp::Max => Some("Lighten"),
        }
    }
}

fn height(g: &Graph, n: NodeId) -> f32 {
    HEADER + (inputs(g, n).len() + outputs(g, n).len()).max(1) as f32 * ROW + 6.0
}

fn out_type(g: &Graph, n: NodeId, output: &str) -> SocketType {
    g.output_type(&graph::Link { node: n, output: output.to_string() }).unwrap_or(SocketType::Float)
}

fn in_type(g: &Graph, n: NodeId, input: &str) -> Option<SocketType> {
    inputs(g, n).into_iter().find(|(name, _, _)| name == input).map(|(_, _, t)| t)
}

/// Every link reading from `src`: (consumer, consumer input, output of `src`), across the whole
/// graph plus the layer sink.
pub fn consumers(g: &Graph, layer: Option<LayerRef>, src: NodeId) -> Vec<(NodeId, String, String)> {
    let mut readers: Vec<NodeId> = g.nodes.keys().copied().collect();
    if layer.is_some() {
        readers.push(SINK);
    }
    let mut v = Vec::new();
    for r in readers {
        for (input, s, out) in links_into(g, layer, r) {
            if s == src {
                v.push((r, input, out));
            }
        }
    }
    v
}

/// The (output, input) pairs that could connect `a` to `b`, best first: exact type matches
/// before conversions, free inputs before linked ones.
pub fn link_options(g: &Graph, layer: Option<LayerRef>, a: NodeId, b: NodeId) -> Vec<(String, String)> {
    if a == b || b != SINK && g.depends_on(a, b) {
        return Vec::new();
    }
    let linked: Vec<String> = links_into(g, layer, b).into_iter().map(|(i, _, _)| i).collect();
    let mut opts = Vec::new();
    for (out, ot) in outputs(g, a) {
        for (input, _, it) in inputs(g, b) {
            if compatible(ot, it) {
                let score = (ot != it) as u8 * 2 + linked.contains(&input) as u8;
                opts.push((score, out.clone(), input));
            }
        }
    }
    opts.sort_by_key(|(s, _, _)| *s);
    opts.into_iter().map(|(_, o, i)| (o, i)).collect()
}

/// Lazy connect: links `a` to `b` by the best matching sockets.
pub fn lazy_connect(g: &mut Graph, layer: Option<LayerRef>, a: NodeId, b: NodeId) -> bool {
    let Some((out, input)) = link_options(g, layer, a, b).into_iter().next() else { return false };
    connect(g, layer, &SocketRef { node: a, name: out }, &SocketRef { node: b, name: input })
}

/// Merges the main outputs of `nodes` (top to bottom) with Math / Merge nodes, or a Stack for
/// materials. Returns the final node.
pub fn merge(g: &mut Graph, layer: Option<LayerRef>, nodes: &[NodeId], op: MergeOp) -> Option<NodeId> {
    let mut nodes: Vec<NodeId> = nodes.iter().copied().filter(|n| *n != SINK && !outputs(g, *n).is_empty()).collect();
    if nodes.len() < 2 {
        return None;
    }
    nodes.sort_by(|a, b| g.node(*a).pos[1].total_cmp(&g.node(*b).pos[1]));
    let main = |g: &Graph, n: NodeId| {
        let (o, t) = outputs(g, n)[0].clone();
        (o, t)
    };
    let types: Vec<SocketType> = nodes.iter().map(|n| main(g, *n).1).collect();
    let x = nodes.iter().map(|n| g.node(*n).pos[0]).fold(f32::MIN, f32::max) + NODE_W + 60.0;
    let y = nodes.iter().map(|n| g.node(*n).pos[1]).sum::<f32>() / nodes.len() as f32;

    if types.iter().all(|t| *t == SocketType::Material) {
        if op != MergeOp::Mix {
            return None;
        }
        // Topmost node becomes the top layer.
        let stack = g.add("stack");
        g.node_mut(stack).pos = [x, y];
        for n in nodes.iter().rev() {
            connect(g, layer, &SocketRef { node: *n, name: main(g, *n).0 }, &SocketRef { node: stack, name: NEW_LAYER.into() });
        }
        return Some(stack);
    }
    if types.contains(&SocketType::Material) {
        return None;
    }
    let all_float = types.iter().all(|t| *t == SocketType::Float);
    let use_math = (all_float && op != MergeOp::Mix) || op.blend().is_none();
    let mut prev = SocketRef { node: nodes[0], name: main(g, nodes[0]).0 };
    let mut last = None;
    for (i, n) in nodes.iter().enumerate().skip(1) {
        let m = if use_math {
            let m = g.add("math");
            g.set(m, "op", Value::Text(op.math().unwrap_or("Add").into()));
            m
        } else {
            let m = g.add("merge");
            g.set(m, "mode", Value::Text(op.blend().unwrap_or("Normal").into()));
            m
        };
        g.node_mut(m).pos = [x + (i - 1) as f32 * (NODE_W + 40.0), y];
        connect(g, layer, &prev, &SocketRef { node: m, name: "a".into() });
        connect(g, layer, &SocketRef { node: *n, name: main(g, *n).0 }, &SocketRef { node: m, name: "b".into() });
        // Float merges continue from the Merge node's float output.
        let out = if !use_math && all_float { "float" } else { "out" };
        prev = SocketRef { node: m, name: out.into() };
        last = Some(m);
    }
    last
}

/// With two nodes: swaps everything they feed. With one: swaps its first two linked inputs
/// (or moves a single link to the next compatible input).
pub fn swap(g: &mut Graph, layer: Option<LayerRef>, nodes: &[NodeId]) -> bool {
    match nodes {
        [a, b] => {
            let (a, b) = (*a, *b);
            let ca = consumers(g, layer, a);
            let cb = consumers(g, layer, b);
            // Each consumer gets the other node's output of the same type, or its main output.
            let pick = |g: &Graph, n: NodeId, src: NodeId, out: &str| {
                let t = out_type(g, src, out);
                outputs(g, n).into_iter().find(|(_, ot)| *ot == t).or_else(|| outputs(g, n).into_iter().next()).map(|(o, _)| o)
            };
            let mut changed = false;
            for (c, input, out) in &ca {
                if let Some(o) = pick(g, b, a, out) {
                    changed |= connect(g, layer, &SocketRef { node: b, name: o }, &SocketRef { node: *c, name: input.clone() });
                }
            }
            for (c, input, out) in &cb {
                if let Some(o) = pick(g, a, b, out) {
                    changed |= connect(g, layer, &SocketRef { node: a, name: o }, &SocketRef { node: *c, name: input.clone() });
                }
            }
            changed
        }
        [n] => {
            let n = *n;
            let links: Vec<(String, NodeId, String)> = links_into(g, layer, n).into_iter().filter(|(i, _, _)| !i.contains('.')).collect();
            match links.as_slice() {
                [(i1, s1, o1), (i2, s2, o2), ..] => {
                    let (i1, s1, o1, i2, s2, o2) = (i1.clone(), *s1, o1.clone(), i2.clone(), *s2, o2.clone());
                    super::disconnect(g, layer, &SocketRef { node: n, name: i1.clone() });
                    super::disconnect(g, layer, &SocketRef { node: n, name: i2.clone() });
                    connect(g, layer, &SocketRef { node: s2, name: o2 }, &SocketRef { node: n, name: i1 });
                    connect(g, layer, &SocketRef { node: s1, name: o1 }, &SocketRef { node: n, name: i2 })
                }
                [(i1, s1, o1)] => {
                    let t = out_type(g, *s1, o1);
                    let Some((next, _, _)) = inputs(g, n).into_iter().find(|(name, _, it)| name != i1 && compatible(t, *it) && !name.ends_with("_new")) else {
                        return false;
                    };
                    let (i1, s1, o1) = (i1.clone(), *s1, o1.clone());
                    super::disconnect(g, layer, &SocketRef { node: n, name: i1 });
                    connect(g, layer, &SocketRef { node: s1, name: o1 }, &SocketRef { node: n, name: next })
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Removes a node, linking what fed it straight into what it fed (Blender's Ctrl+X).
pub fn dissolve(g: &mut Graph, layer: Option<LayerRef>, n: NodeId) {
    let feeds = links_into(g, layer, n);
    for (c, input, out) in consumers(g, layer, n) {
        let want = out_type(g, n, &out);
        let Some(target) = in_type(g, c, &input) else { continue };
        // Prefer an input carrying the same type as the output being replaced.
        let best = feeds
            .iter()
            .filter(|(_, s, o)| compatible(out_type(g, *s, o), target))
            .min_by_key(|(_, s, o)| (out_type(g, *s, o) != want) as u8)
            .cloned();
        if let Some((_, s, o)) = best {
            connect(g, layer, &SocketRef { node: s, name: o }, &SocketRef { node: c, name: input });
        }
    }
    g.remove(n);
}

/// Copies nodes (with their parameters and input links), offset down and right.
pub fn duplicate(g: &mut Graph, nodes: &[NodeId]) -> Vec<NodeId> {
    let mut map = BTreeMap::new();
    for &n in nodes.iter().filter(|n| **n != SINK) {
        let src = g.node(n).clone();
        if src.ty == "material_output" || n == g.root {
            continue;
        }
        let id = g.add(&src.ty);
        let node = g.node_mut(id);
        node.label = src.label;
        node.params = src.params;
        node.cached = src.cached;
        node.inputs = src.inputs;
        node.slots = src.slots;
        node.mask_slots = src.mask_slots;
        node.pos = [src.pos[0] + 30.0, src.pos[1] + 30.0];
        map.insert(n, id);
    }
    // Links between duplicated nodes point at the copies.
    for &id in map.values() {
        let node = g.node_mut(id);
        for l in node.inputs.values_mut() {
            if let Some(c) = map.get(&l.node) {
                l.node = *c;
            }
        }
    }
    map.into_values().collect()
}

/// Lines nodes up in a row (if spread horizontally) or a column, evenly spaced.
pub fn align(g: &mut Graph, nodes: &[NodeId]) {
    let mut nodes: Vec<NodeId> = nodes.iter().copied().filter(|n| *n != SINK).collect();
    if nodes.len() < 2 {
        return;
    }
    let xs: Vec<f32> = nodes.iter().map(|n| g.node(*n).pos[0]).collect();
    let ys: Vec<f32> = nodes.iter().map(|n| g.node(*n).pos[1]).collect();
    let spread = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max) - v.iter().copied().fold(f32::MAX, f32::min);
    if spread(&xs) > spread(&ys) {
        let y = ys.iter().sum::<f32>() / ys.len() as f32;
        nodes.sort_by(|a, b| g.node(*a).pos[0].total_cmp(&g.node(*b).pos[0]));
        let mut x = g.node(nodes[0]).pos[0];
        for n in nodes {
            g.node_mut(n).pos = [x, y];
            x += NODE_W + 50.0;
        }
    } else {
        let x = xs.iter().sum::<f32>() / xs.len() as f32;
        nodes.sort_by(|a, b| g.node(*a).pos[1].total_cmp(&g.node(*b).pos[1]));
        let mut y = g.node(nodes[0]).pos[1];
        for n in nodes {
            let h = height(g, n);
            g.node_mut(n).pos = [x, y];
            y += h + 20.0;
        }
    }
}

/// Resets parameters to their defaults.
pub fn reset(g: &mut Graph, n: NodeId) {
    if n == SINK {
        return;
    }
    let def = g.node(n).def();
    let params = def.params.iter().map(|p| (p.name.to_string(), (p.default)())).collect();
    g.node_mut(n).params = params;
}

/// Copies the parameters of `from` onto the other nodes of the same type. Returns how many.
pub fn copy_settings(g: &mut Graph, from: NodeId, to: &[NodeId]) -> usize {
    let src = g.node(from).clone();
    let mut count = 0;
    for &n in to.iter().filter(|n| **n != from && **n != SINK) {
        if g.node(n).ty == src.ty {
            g.node_mut(n).params = src.params.clone();
            count += 1;
        }
    }
    count
}

/// Links the active node into each other selected node (Blender's K / Shift+K).
pub fn link_to_selected(g: &mut Graph, layer: Option<LayerRef>, active: NodeId, others: &[NodeId]) -> usize {
    let mut count = 0;
    for &n in others.iter().filter(|n| **n != active) {
        if lazy_connect(g, layer, active, n) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(g: &Graph, n: NodeId, input: &str) -> Option<(NodeId, String)> {
        g.node(n).inputs.get(input).map(|l| (l.node, l.output.clone()))
    }

    #[test]
    fn merge_floats_with_math() {
        let mut g = Graph::new();
        let (a, b) = (g.add("noise"), g.add("cellular"));
        g.node_mut(b).pos = [0.0, 100.0];
        let m = merge(&mut g, None, &[a, b], MergeOp::Add).unwrap();
        assert_eq!(g.node(m).ty, "math");
        assert_eq!(g.node(m).params["op"], Value::Text("Add".into()));
        assert_eq!(link(&g, m, "a"), Some((a, "out".into())));
        assert_eq!(link(&g, m, "b"), Some((b, "out".into())));
    }

    #[test]
    fn merge_three_chains_and_mix_uses_merge_node() {
        let mut g = Graph::new();
        let ns: Vec<NodeId> = (0..3).map(|i| {
            let n = g.add("noise");
            g.node_mut(n).pos = [0.0, i as f32 * 100.0];
            n
        }).collect();
        let last = merge(&mut g, None, &ns, MergeOp::Mix).unwrap();
        assert_eq!(g.node(last).ty, "merge");
        let (first, out) = link(&g, last, "a").unwrap();
        assert_eq!(g.node(first).ty, "merge");
        assert_eq!(out, "float");
        assert_eq!(link(&g, last, "b"), Some((ns[2], "out".into())));
    }

    #[test]
    fn merge_materials_makes_a_stack() {
        let mut g = Graph::new();
        let (a, b) = (g.add("fill"), g.add("fill"));
        g.node_mut(b).pos = [0.0, 100.0];
        let s = merge(&mut g, None, &[a, b], MergeOp::Mix).unwrap();
        let slots: Vec<Option<NodeId>> = g.node(s).slots.iter().map(|s| s.content).collect();
        // The upper node is the top (last) layer.
        assert_eq!(slots, vec![Some(b), Some(a)]);
        assert!(merge(&mut g, None, &[a, b], MergeOp::Add).is_none());
    }

    #[test]
    fn dissolve_reconnects() {
        let mut g = Graph::new();
        let (n, lv, r) = (g.add("noise"), g.add("levels"), g.add("ramp"));
        assert!(g.connect(n, "out", lv, "in"));
        assert!(g.connect(lv, "float", r, "in"));
        dissolve(&mut g, None, lv);
        assert!(g.get(lv).is_none());
        assert_eq!(link(&g, r, "in"), Some((n, "out".into())));
    }

    #[test]
    fn swap_inputs_and_outputs() {
        let mut g = Graph::new();
        let (a, b, m) = (g.add("noise"), g.add("cellular"), g.add("math"));
        g.connect(a, "out", m, "a");
        g.connect(b, "out", m, "b");
        assert!(swap(&mut g, None, &[m]));
        assert_eq!(link(&g, m, "a"), Some((b, "out".into())));
        assert_eq!(link(&g, m, "b"), Some((a, "out".into())));
        // Two nodes: what read `a` now reads `b` and vice versa.
        assert!(swap(&mut g, None, &[a, b]));
        assert_eq!(link(&g, m, "a"), Some((a, "out".into())));
        assert_eq!(link(&g, m, "b"), Some((b, "out".into())));
    }

    #[test]
    fn lazy_connect_prefers_exact_types_and_refuses_cycles() {
        let mut g = Graph::new();
        let (n, lv) = (g.add("noise"), g.add("levels"));
        assert!(lazy_connect(&mut g, None, n, lv));
        assert_eq!(link(&g, lv, "in"), Some((n, "out".into())));
        assert!(link_options(&g, None, lv, n).is_empty());
        let r = g.add("ramp");
        assert!(lazy_connect(&mut g, None, lv, r));
        // Ramp takes a float: the float output is the exact match.
        assert_eq!(link(&g, r, "in"), Some((lv, "float".into())));
    }

    #[test]
    fn duplicate_remaps_internal_links() {
        let mut g = Graph::new();
        let (n, lv) = (g.add("noise"), g.add("levels"));
        g.connect(n, "out", lv, "in");
        let copies = duplicate(&mut g, &[n, lv]);
        assert_eq!(copies.len(), 2);
        let (cn, clv) = (copies[0], copies[1]);
        assert_eq!(link(&g, clv, "in"), Some((cn, "out".into())));
        // Duplicating only the reader keeps its link to the original.
        let only = duplicate(&mut g, &[lv]);
        assert_eq!(link(&g, only[0], "in"), Some((n, "out".into())));
    }

    #[test]
    fn reset_and_copy_settings() {
        let mut g = Graph::new();
        let (a, b, c) = (g.add("noise"), g.add("noise"), g.add("cellular"));
        g.set(a, "scale", Value::Float(42.0));
        assert_eq!(copy_settings(&mut g, a, &[a, b, c]), 1);
        assert_eq!(g.node(b).params["scale"], Value::Float(42.0));
        reset(&mut g, b);
        assert_ne!(g.node(b).params["scale"], Value::Float(42.0));
    }
}
