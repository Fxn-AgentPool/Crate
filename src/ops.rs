//! Document operations on the layer stack, shared by the UI and the script runner.
//! See docs/reference/layers.md "Layer operations".

use crate::graph::{BlendMode, Graph, MaskSlot, MaskSource, NodeId, Slot, Value};

/// A layer: slot `index` of Stack node `stack`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerRef {
    pub stack: NodeId,
    pub index: usize,
}

/// What is selected in the Layers panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Layer(LayerRef),
    /// A mask layer: slot `mask_index` of the layer's Mask Stack.
    MaskLayer(LayerRef, usize),
}

impl Selection {
    pub fn layer(&self) -> LayerRef {
        match *self {
            Selection::Layer(l) | Selection::MaskLayer(l, _) => l,
        }
    }
}

pub fn slot(g: &Graph, l: LayerRef) -> Option<&Slot> {
    g.get(l.stack)?.slots.get(l.index)
}

pub fn slot_mut(g: &mut Graph, l: LayerRef) -> Option<&mut Slot> {
    g.nodes.get_mut(&l.stack)?.slots.get_mut(l.index)
}

/// Kinds shown in the Layers panel, derived from the slot content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    Paint,
    Fill,
    Group,
    Graph,
    SmartMaterial,
}

impl LayerKind {
    pub fn name(self) -> &'static str {
        match self {
            LayerKind::Paint => "Paint layer",
            LayerKind::Fill => "Fill layer",
            LayerKind::Group => "Group",
            LayerKind::Graph => "Graph layer",
            LayerKind::SmartMaterial => "Smart material",
        }
    }
}

pub fn layer_kind(g: &Graph, s: &Slot) -> LayerKind {
    if s.smart_material.is_some() {
        return LayerKind::SmartMaterial;
    }
    match s.content.and_then(|c| g.get(c)).map(|n| n.ty.as_str()) {
        Some("paint") => LayerKind::Paint,
        Some("fill") => LayerKind::Fill,
        Some("stack") => LayerKind::Group,
        _ => LayerKind::Graph,
    }
}

fn unique_name(g: &Graph, stack: NodeId, base: &str) -> String {
    let names: Vec<&str> = g.node(stack).slots.iter().map(|s| s.name.as_str()).collect();
    if !names.contains(&base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !names.contains(&n.as_str())).unwrap()
}

/// Index where a new layer goes: above `above`, or at the top of the root stack.
fn insert_point(g: &Graph, above: Option<LayerRef>) -> LayerRef {
    match above {
        Some(l) if g.get(l.stack).is_some() => LayerRef { stack: l.stack, index: (l.index + 1).min(g.node(l.stack).slots.len()) },
        _ => LayerRef { stack: g.root, index: g.node(g.root).slots.len() },
    }
}

fn insert(g: &mut Graph, at: LayerRef, slot: Slot) -> LayerRef {
    g.node_mut(at.stack).slots.insert(at.index, slot);
    at
}

/// Slots are stored bottom to top (index 0 is the bottom layer).
pub fn add_paint_layer(g: &mut Graph, above: Option<LayerRef>) -> LayerRef {
    let at = insert_point(g, above);
    let node = g.add("paint");
    let name = unique_name(g, at.stack, "Paint");
    insert(g, at, Slot::new(name, node))
}

pub fn add_fill_layer(g: &mut Graph, above: Option<LayerRef>) -> LayerRef {
    let at = insert_point(g, above);
    let node = g.add("fill");
    let name = unique_name(g, at.stack, "Fill");
    let mut slot = Slot::new(name, node);
    // The layer affects the channels the fill outputs; the Properties panel keeps them in step.
    for c in crate::channels::Channel::ALL {
        slot.channels[c.index()].enabled = g.node(node).param(crate::graph::fill_param_names(c).0).as_bool();
    }
    insert(g, at, slot)
}

/// Turns a channel of a layer on or off. On a Fill layer it also turns the Fill node's output for
/// the channel on or off, so the two stay in step (docs/reference/layers.md#properties-panel).
pub fn set_channel_enabled(g: &mut Graph, l: LayerRef, c: crate::channels::Channel, on: bool) {
    let Some(s) = slot_mut(g, l) else { return };
    s.channels[c.index()].enabled = on;
    let content = s.content;
    if let Some(n) = content.filter(|n| g.get(*n).is_some_and(|n| n.ty == "fill")) {
        g.set(n, crate::graph::fill_param_names(c).0, Value::Bool(on));
    }
}

/// Adds an empty folder (a Group with no layers).
pub fn add_folder(g: &mut Graph, above: Option<LayerRef>) -> LayerRef {
    let at = insert_point(g, above);
    let node = g.add("stack");
    let name = unique_name(g, at.stack, "Folder");
    let mut slot = Slot::new(name, node);
    slot.expanded = true;
    insert(g, at, slot)
}

/// Inserts an existing content node (e.g. a smart material subgraph) as a layer.
pub fn add_layer_with(g: &mut Graph, above: Option<LayerRef>, name: &str, content: NodeId) -> LayerRef {
    let at = insert_point(g, above);
    let name = unique_name(g, at.stack, name);
    insert(g, at, Slot::new(name, content))
}

/// Moves the given layers (all in one stack) into a new group at the topmost position.
pub fn group(g: &mut Graph, layers: &[LayerRef]) -> Option<LayerRef> {
    let stack = layers.first()?.stack;
    if layers.iter().any(|l| l.stack != stack) {
        return None;
    }
    let len = g.get(stack)?.slots.len();
    let mut idx: Vec<usize> = layers.iter().map(|l| l.index).filter(|i| *i < len).collect();
    idx.sort_unstable();
    idx.dedup();
    let top = *idx.last()?;
    let group = g.add("stack");
    let mut moved = Vec::new();
    for &i in idx.iter().rev() {
        moved.push(g.node_mut(stack).slots.remove(i));
    }
    moved.reverse();
    // Their Node Editor positions were next to the old stack: placed anew.
    for s in &mut moved {
        s.node_pos = [0.0, 0.0];
    }
    g.node_mut(group).slots = moved;
    let at = top + 1 - idx.len();
    let name = unique_name(g, stack, "Group");
    let mut slot = Slot::new(name, group);
    slot.expanded = true;
    g.node_mut(stack).slots.insert(at, slot);
    Some(LayerRef { stack, index: at })
}

pub fn delete(g: &mut Graph, l: LayerRef) {
    if l.index < g.node(l.stack).slots.len() {
        g.node_mut(l.stack).slots.remove(l.index);
        g.collect_garbage();
    }
}

/// Duplicates a layer (content and mask). Returns the new layer and the node id mapping,
/// so paint data can be copied.
pub fn duplicate(g: &mut Graph, l: LayerRef) -> Option<(LayerRef, Vec<(NodeId, NodeId)>)> {
    let mut slot = slot(g, l)?.clone();
    let mut pairs = Vec::new();
    if let Some(c) = slot.content {
        let (nc, map) = g.duplicate(c);
        slot.content = Some(nc);
        pairs.extend(map);
    }
    if let Some(m) = slot.mask {
        let (nm, map) = g.duplicate(m);
        slot.mask = Some(nm);
        pairs.extend(map);
    }
    slot.name = unique_name(g, l.stack, &format!("{} copy", slot.name));
    // Placed anew in the Node Editor rather than on top of the original.
    slot.node_pos = [0.0, 0.0];
    let at = LayerRef { stack: l.stack, index: l.index + 1 };
    g.node_mut(l.stack).slots.insert(at.index, slot);
    Some((at, pairs))
}

/// Moves layer `l` to directly below `target` (possibly in another stack).
pub fn move_below(g: &mut Graph, l: LayerRef, target: LayerRef) -> LayerRef {
    move_to(g, l, target, 0)
}

pub fn move_above(g: &mut Graph, l: LayerRef, target: LayerRef) -> LayerRef {
    move_to(g, l, target, 1)
}

fn move_to(g: &mut Graph, l: LayerRef, target: LayerRef, offset: usize) -> LayerRef {
    if l == target || slot(g, l).is_none() || slot(g, target).is_none() {
        return l;
    }
    // A group must not be moved into itself.
    if let Some(c) = slot(g, l).and_then(|s| s.content) {
        if c == target.stack || g.depends_on(c, target.stack) {
            return l;
        }
    }
    let mut s = g.node_mut(l.stack).slots.remove(l.index);
    if l.stack != target.stack {
        s.node_pos = [0.0, 0.0];
    }
    let mut index = target.index;
    if l.stack == target.stack && l.index < target.index {
        index -= 1;
    }
    let index = (index + offset).min(g.node(target.stack).slots.len());
    g.node_mut(target.stack).slots.insert(index, s);
    LayerRef { stack: target.stack, index }
}

fn new_mask_stack(g: &mut Graph, value: f32) -> NodeId {
    let ms = g.add("mask_stack");
    let fill = g.add("value");
    g.set(fill, "type", Value::Text("Factor".into()));
    g.set(fill, "factor", Value::Float(value));
    g.node_mut(ms).mask_slots.push(MaskSlot {
        name: if value > 0.5 { "White".into() } else { "Black".into() },
        visible: true,
        opacity: 1.0,
        mode: BlendMode::Normal,
        source: MaskSource::Node(fill),
    });
    ms
}

/// Adds a mask: white (fully visible) or black (hidden). Replaces an existing mask.
pub fn add_mask(g: &mut Graph, l: LayerRef, black: bool) {
    let ms = new_mask_stack(g, if black { 0.0 } else { 1.0 });
    if let Some(s) = slot_mut(g, l) {
        s.mask = Some(ms);
        s.expanded = true;
    }
    g.collect_garbage();
}

pub fn remove_mask(g: &mut Graph, l: LayerRef) {
    if let Some(s) = slot_mut(g, l) {
        s.mask = None;
    }
    g.collect_garbage();
}

fn ensure_mask(g: &mut Graph, l: LayerRef) -> NodeId {
    if let Some(m) = slot(g, l).and_then(|s| s.mask) {
        return m;
    }
    let ms = new_mask_stack(g, 1.0);
    slot_mut(g, l).unwrap().mask = Some(ms);
    ms
}

fn push_mask_slot(g: &mut Graph, l: LayerRef, name: &str, source: MaskSource) -> Selection {
    let ms = ensure_mask(g, l);
    if let Some(s) = slot_mut(g, l) {
        s.expanded = true;
    }
    let slots = &mut g.node_mut(ms).mask_slots;
    slots.push(MaskSlot { name: name.into(), visible: true, opacity: 1.0, mode: BlendMode::Normal, source });
    Selection::MaskLayer(l, slots.len() - 1)
}

pub fn add_mask_generator(g: &mut Graph, l: LayerRef, ty: &str) -> Selection {
    let node = g.add(ty);
    let name = g.node(node).def().title;
    push_mask_slot(g, l, name, MaskSource::Node(node))
}

/// Adds a Mesh Map node (for example Curvature) as a mask effect.
pub fn add_mask_mesh_map(g: &mut Graph, l: LayerRef, map: crate::meshmaps::MeshMap) -> Selection {
    let node = g.add("mesh_map");
    g.set(node, "map", crate::graph::Value::Text(map.param_name().into()));
    push_mask_slot(g, l, map.name(), MaskSource::Node(node))
}

pub fn add_mask_paint(g: &mut Graph, l: LayerRef) -> Selection {
    let node = g.add("paint_mask");
    push_mask_slot(g, l, "Paint", MaskSource::Node(node))
}

pub fn add_mask_fill(g: &mut Graph, l: LayerRef) -> Selection {
    let node = g.add("value");
    g.set(node, "type", Value::Text("Factor".into()));
    push_mask_slot(g, l, "Fill", MaskSource::Node(node))
}

pub fn add_mask_levels(g: &mut Graph, l: LayerRef) -> Selection {
    push_mask_slot(
        g,
        l,
        "Levels",
        MaskSource::Levels { in_black: 0.0, in_white: 1.0, gamma: 1.0, out_black: 0.0, out_white: 1.0 },
    )
}

pub fn mask_slot_mut(g: &mut Graph, l: LayerRef, i: usize) -> Option<&mut MaskSlot> {
    let ms = slot(g, l)?.mask?;
    g.nodes.get_mut(&ms)?.mask_slots.get_mut(i)
}

pub fn mask_slot(g: &Graph, l: LayerRef, i: usize) -> Option<&MaskSlot> {
    let ms = slot(g, l)?.mask?;
    g.get(ms)?.mask_slots.get(i)
}

pub fn delete_mask_layer(g: &mut Graph, l: LayerRef, i: usize) {
    if let Some(ms) = slot(g, l).and_then(|s| s.mask) {
        let slots = &mut g.node_mut(ms).mask_slots;
        if i < slots.len() {
            slots.remove(i);
        }
        g.collect_garbage();
    }
}

/// The node whose parameters the Properties panel edits for a selection.
pub fn selected_node(g: &Graph, sel: Selection) -> Option<NodeId> {
    match sel {
        Selection::Layer(l) => slot(g, l)?.content,
        Selection::MaskLayer(l, i) => match &mask_slot(g, l, i)?.source {
            MaskSource::Node(n) => Some(*n),
            MaskSource::Levels { .. } => None,
        },
    }
}

/// The Paint or Paint Mask node painting writes to, if the selection is paintable.
pub fn paint_target(g: &Graph, sel: Selection) -> Option<NodeId> {
    let n = selected_node(g, sel)?;
    match g.get(n)?.ty.as_str() {
        "paint" | "paint_mask" => Some(n),
        _ => None,
    }
}

/// Finds a layer by name (depth-first through groups).
pub fn find_layer(g: &Graph, name: &str) -> Option<LayerRef> {
    fn walk(g: &Graph, stack: NodeId, name: &str) -> Option<LayerRef> {
        for (i, s) in g.get(stack)?.slots.iter().enumerate() {
            if s.name == name {
                return Some(LayerRef { stack, index: i });
            }
            if let Some(c) = s.content {
                if g.get(c).is_some_and(|n| n.ty == "stack") {
                    if let Some(r) = walk(g, c, name) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }
    walk(g, g.root, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::Channel;

    #[test]
    fn fill_layer_channels_follow_fill_outputs() {
        let mut g = Graph::new();
        let l = add_fill_layer(&mut g, None);
        let fill = slot(&g, l).unwrap().content.unwrap();
        let on = |g: &Graph, c: Channel| (slot(g, l).unwrap().channels[c.index()].enabled, g.node(fill).param(crate::graph::fill_param_names(c).0).as_bool());
        // A new fill affects the channels it outputs: Base Color, Roughness and Metallic.
        assert_eq!(on(&g, Channel::Roughness), (true, true));
        assert_eq!(on(&g, Channel::Height), (false, false));
        set_channel_enabled(&mut g, l, Channel::Height, true);
        assert_eq!(on(&g, Channel::Height), (true, true));
        set_channel_enabled(&mut g, l, Channel::Roughness, false);
        assert_eq!(on(&g, Channel::Roughness), (false, false));
    }
}
