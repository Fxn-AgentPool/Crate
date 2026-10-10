//! Layers and Properties panels. See docs/reference/layers.md and windows-and-panels.md.

use egui::{Color32, RichText};

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::graph::{self, BlendMode, Graph, MaskSource, NodeId, ParamDef, ParamKind, Value};
use crate::ops::{self, LayerKind, LayerRef, Selection};
use crate::project::Project;

/// Requests from the panels that the app handles (they need more than the project).
#[derive(Default)]
pub struct PanelActions {
    pub edit_as_graph: Option<NodeId>,
    pub save_smart_material: Option<LayerRef>,
    pub insert_smart_material: Option<std::path::PathBuf>,
}


pub(crate) fn rename_popup(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef) {
    let id = egui::Id::new("rename");
    let Some((stack, index, mut text)) = ui.ctx().data(|d| d.get_temp::<(NodeId, usize, String)>(id)) else { return };
    if (stack, index) != (l.stack, l.index) {
        return;
    }
    let resp = ui.text_edit_singleline(&mut text);
    resp.request_focus();
    if resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ui.ctx().data_mut(|d| d.remove::<(NodeId, usize, String)>(id));
        if !text.trim().is_empty() {
            p.edit(gpu, "Rename layer", |g| {
                if let Some(s) = ops::slot_mut(g, l) {
                    s.name = text.trim().to_string();
                }
            });
        }
    } else {
        ui.ctx().data_mut(|d| d.insert_temp(id, (stack, index, text)));
    }
}

pub(crate) fn layer_context_menu(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef, actions: &mut PanelActions) {
    let has_mask = ops::slot(&p.graph, l).is_some_and(|s| s.mask.is_some());
    if ui.button("Add mask").clicked() {
        p.edit(gpu, "Add mask", |g| ops::add_mask(g, l, false));
        expand(p, l);
        ui.close();
    }
    if ui.button("Add black mask").clicked() {
        p.edit(gpu, "Add black mask", |g| ops::add_mask(g, l, true));
        expand(p, l);
        ui.close();
    }
    if has_mask {
        mask_add_menu(ui, gpu, p, l);
        if ui.button("Remove mask").clicked() {
            p.edit(gpu, "Remove mask", |g| ops::remove_mask(g, l));
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Edit as graph\tG").clicked() {
        actions.edit_as_graph = ops::slot(&p.graph, l).and_then(|s| s.content);
        ui.close();
    }
    let kind = ops::slot(&p.graph, l).map(|s| ops::layer_kind(&p.graph, s));
    if matches!(kind, Some(LayerKind::Group | LayerKind::Graph | LayerKind::SmartMaterial)) {
        if ui.button("Save as smart material").clicked() {
            actions.save_smart_material = Some(l);
            ui.close();
        }
    }
    if ui.button("Group\tCtrl+G").clicked() {
        group_selected(gpu, p);
        ui.close();
    }
    if ui.button("Duplicate\tCtrl+D").clicked() {
        duplicate_selected(gpu, p);
        ui.close();
    }
    if ui.button("Delete\tDelete").clicked() {
        delete_selected(gpu, p);
        ui.close();
    }
}

fn expand(p: &mut Project, l: LayerRef) {
    if let Some(s) = ops::slot_mut(&mut p.graph, l) {
        s.expanded = true;
    }
}

pub(crate) fn mask_add_menu(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef) {
    ui.menu_button("Add generator", |ui| {
        for ty in graph::GENERATORS {
            if ui.button(graph::def(ty).unwrap().title).clicked() {
                let sel = p.edit(gpu, "Add generator", |g| ops::add_mask_generator(g, l, ty));
                p.selection = Some(sel);
                expand(p, l);
                ui.close();
            }
        }
    });
    if ui.button("Add paint").clicked() {
        let sel = p.edit(gpu, "Add mask paint", |g| ops::add_mask_paint(g, l));
        p.selection = Some(sel);
        expand(p, l);
        ui.close();
    }
    if ui.button("Add fill").clicked() {
        let sel = p.edit(gpu, "Add mask fill", |g| ops::add_mask_fill(g, l));
        p.selection = Some(sel);
        expand(p, l);
        ui.close();
    }
    if ui.button("Add levels").clicked() {
        let sel = p.edit(gpu, "Add levels", |g| ops::add_mask_levels(g, l));
        p.selection = Some(sel);
        expand(p, l);
        ui.close();
    }
}

fn kind_color(k: LayerKind) -> Color32 {
    match k {
        LayerKind::Paint => Color32::from_rgb(150, 90, 40),
        LayerKind::Fill => Color32::from_rgb(50, 110, 60),
        LayerKind::Group => Color32::from_rgb(90, 90, 110),
        LayerKind::Graph => Color32::from_rgb(80, 70, 130),
        LayerKind::SmartMaterial => Color32::from_rgb(140, 110, 30),
    }
}

/// Small colored label used as an icon.
fn badge(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(color)
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(4, 0))
        .show(ui, |ui| ui.label(RichText::new(text).small().strong().color(Color32::WHITE)))
        .response
}

// ------------------------------------------------------------------ commands

pub fn add_paint(gpu: &Gpu, p: &mut Project) {
    let above = p.selection.map(|s| s.layer());
    let l = p.edit(gpu, "New paint layer", |g| ops::add_paint_layer(g, above));
    p.selection = Some(Selection::Layer(l));
}

pub fn add_fill(gpu: &Gpu, p: &mut Project) {
    let above = p.selection.map(|s| s.layer());
    let l = p.edit(gpu, "New fill layer", |g| ops::add_fill_layer(g, above));
    p.selection = Some(Selection::Layer(l));
}

pub fn group_selected(gpu: &Gpu, p: &mut Project) {
    let Some(sel) = p.selection else { return };
    let mut layers = p.multi.clone();
    layers.push(sel.layer());
    if let Some(l) = p.edit(gpu, "Group", |g| ops::group(g, &layers)) {
        p.selection = Some(Selection::Layer(l));
    }
    p.multi.clear();
}

pub fn duplicate_selected(gpu: &Gpu, p: &mut Project) {
    let Some(sel) = p.selection else { return };
    if let Some((l, pairs)) = p.edit(gpu, "Duplicate", |g| ops::duplicate(g, sel.layer())) {
        p.copy_paint(gpu, &pairs);
        p.selection = Some(Selection::Layer(l));
    }
}

pub fn delete_selected(gpu: &Gpu, p: &mut Project) {
    let Some(sel) = p.selection else { return };
    match sel {
        Selection::Layer(l) => {
            // Delete the Ctrl+click selection too, highest index first so indices stay valid.
            let mut layers: Vec<LayerRef> = p.multi.iter().copied().filter(|m| m.stack == l.stack).collect();
            layers.push(l);
            layers.sort_by_key(|m| std::cmp::Reverse(m.index));
            layers.dedup();
            let l = *layers.last().unwrap();
            p.edit(gpu, "Delete layer", |g| {
                for m in &layers {
                    ops::delete(g, *m);
                }
            });
            let len = p.graph.get(l.stack).map(|n| n.slots.len()).unwrap_or(0);
            p.selection = if len == 0 {
                None
            } else {
                Some(Selection::Layer(LayerRef { stack: l.stack, index: l.index.min(len - 1) }))
            };
        }
        Selection::MaskLayer(l, i) => {
            p.edit(gpu, "Delete mask layer", |g| ops::delete_mask_layer(g, l, i));
            p.selection = Some(Selection::Layer(l));
        }
    }
}

pub fn move_selected(gpu: &Gpu, p: &mut Project, delta: i32) {
    let Some(Selection::Layer(l)) = p.selection else { return };
    let l = p.edit(gpu, "Move layer", |g| ops::shift(g, l, delta));
    p.selection = Some(Selection::Layer(l));
}

// ------------------------------------------------------------------ properties

pub fn properties_panel(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) {
    let Some(sel) = p.selection else {
        ui.label(RichText::new("Select a layer.").weak());
        return;
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match sel {
        Selection::Layer(l) => layer_properties(ui, gpu, p, l),
        Selection::MaskLayer(l, i) => mask_layer_properties(ui, gpu, p, l, i),
    });
}

/// Tracks whether a widget interaction should start a new undo step.
fn track(p: &mut Project, gpu: &Gpu, resp: &egui::Response, label: &str, apply: impl FnOnce(&mut Graph)) {
    if resp.drag_started() || (resp.changed() && !resp.dragged()) {
        p.checkpoint(label);
    }
    if resp.changed() {
        apply(&mut p.graph);
        p.graph_changed(gpu);
    }
}

fn layer_properties(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef) {
    let Some(slot) = ops::slot(&p.graph, l).cloned() else { return };
    let kind = ops::layer_kind(&p.graph, &slot);
    ui.horizontal(|ui| {
        badge(ui, kind.icon(), kind_color(kind));
        ui.heading(&slot.name);
    });
    let mut opacity = slot.opacity * 100.0;
    let r = ui.add(egui::Slider::new(&mut opacity, 0.0..=100.0).text("Opacity").suffix(" %"));
    track(p, gpu, &r, "Layer opacity", |g| {
        if let Some(s) = ops::slot_mut(g, l) {
            s.opacity = opacity / 100.0;
        }
    });

    egui::CollapsingHeader::new("Channels").default_open(true).show(ui, |ui| {
        egui::Grid::new("layer channels").num_columns(4).show(ui, |ui| {
            for c in Channel::ALL {
                if !p.channels.is_enabled(c) {
                    continue;
                }
                let cb = slot.channels[c.index()];
                let mut enabled = cb.enabled;
                let r = ui.checkbox(&mut enabled, c.name());
                track(p, gpu, &r, "Layer channel", |g| {
                    if let Some(s) = ops::slot_mut(g, l) {
                        s.channels[c.index()].enabled = enabled;
                    }
                });
                let mut mode = cb.mode;
                let mut changed = false;
                egui::ComboBox::from_id_salt(("mode", c.index())).width(100.0).selected_text(mode.name()).show_ui(ui, |ui| {
                    for m in BlendMode::for_channel(c) {
                        changed |= ui.selectable_value(&mut mode, *m, m.name()).changed();
                    }
                });
                if changed {
                    p.edit(gpu, "Blend mode", |g| {
                        if let Some(s) = ops::slot_mut(g, l) {
                            s.channels[c.index()].mode = mode;
                        }
                    });
                }
                let mut op = cb.opacity * 100.0;
                let r = ui.add(egui::DragValue::new(&mut op).range(0.0..=100.0).suffix(" %"));
                track(p, gpu, &r, "Channel opacity", |g| {
                    if let Some(s) = ops::slot_mut(g, l) {
                        s.channels[c.index()].opacity = op / 100.0;
                    }
                });
                ui.end_row();
            }
        });
    });

    let Some(content) = slot.content else { return };
    match kind {
        LayerKind::Fill => {
            ui.separator();
            fill_properties(ui, gpu, p, content);
        }
        LayerKind::Paint => {
            ui.separator();
            ui.label(RichText::new("Paint with the Paint tool (B).").weak());
        }
        LayerKind::Graph | LayerKind::SmartMaterial | LayerKind::Group => {
            exposed_parameters(ui, gpu, p, content);
        }
    }
}

/// Parameters marked Exposed anywhere in the layer's subgraph.
fn exposed_parameters(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, content: NodeId) {
    let nodes: Vec<NodeId> = p
        .graph
        .subgraph(content)
        .into_iter()
        .filter(|n| !p.graph.node(*n).exposed.is_empty())
        .collect();
    if nodes.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Exposed parameters").strong());
    for n in nodes {
        let node = p.graph.node(n).clone();
        for def in &node.def().params {
            if node.exposed.contains(def.name) {
                param_widget(ui, gpu, p, n, def, &format!("{}: {}", node.title(), def.name));
            }
        }
    }
}

fn fill_properties(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId) {
    let n = p.graph.node(node).clone();
    let defs = &n.def().params;
    for def in defs.iter().take(5) {
        if def.name == "triplanar_blend" && n.param("projection").as_str() != "Triplanar" {
            continue;
        }
        param_widget(ui, gpu, p, node, def, def.name);
    }
    ui.separator();
    egui::Grid::new("fill channels").num_columns(3).show(ui, |ui| {
        for c in Channel::ALL {
            if !p.channels.is_enabled(c) {
                continue;
            }
            let (on, val, src, img) = graph::fill_param_names(c);
            let find = |name: &str| defs.iter().find(|d| d.name == name).unwrap();
            let mut enabled = n.param(on).as_bool();
            let r = ui.checkbox(&mut enabled, c.name());
            track(p, gpu, &r, "Fill channel", |g| g.set(node, on, Value::Bool(enabled)));
            ui.add_enabled_ui(enabled, |ui| {
                if n.param(src).as_str() == "Image" {
                    param_value(ui, gpu, p, node, find(img), img);
                } else {
                    param_value(ui, gpu, p, node, find(val), val);
                }
            });
            ui.add_enabled_ui(enabled, |ui| param_value(ui, gpu, p, node, find(src), src));
            ui.end_row();
        }
    });
}

fn mask_layer_properties(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef, i: usize) {
    let Some(m) = ops::mask_slot(&p.graph, l, i).cloned() else { return };
    ui.horizontal(|ui| {
        badge(ui, "M", Color32::from_gray(90));
        ui.heading(&m.name);
    });
    let mut opacity = m.opacity * 100.0;
    let r = ui.add(egui::Slider::new(&mut opacity, 0.0..=100.0).text("Opacity").suffix(" %"));
    track(p, gpu, &r, "Mask opacity", |g| {
        if let Some(ms) = ops::mask_slot_mut(g, l, i) {
            ms.opacity = opacity / 100.0;
        }
    });
    let mut mode = m.mode;
    let mut changed = false;
    egui::ComboBox::from_label("Blend mode").selected_text(mode.name()).show_ui(ui, |ui| {
        for b in BlendMode::MASK {
            changed |= ui.selectable_value(&mut mode, *b, b.name()).changed();
        }
    });
    if changed {
        p.edit(gpu, "Mask blend mode", |g| {
            if let Some(ms) = ops::mask_slot_mut(g, l, i) {
                ms.mode = mode;
            }
        });
    }
    ui.separator();
    match m.source {
        MaskSource::Levels { in_black, in_white, gamma, out_black, out_white } => {
            let mut v = [in_black, in_white, gamma, out_black, out_white];
            let names = ["In black", "In white", "Gamma", "Out black", "Out white"];
            for k in 0..5 {
                let range = if k == 2 { 0.01..=10.0 } else { 0.0..=1.0 };
                let r = ui.add(egui::Slider::new(&mut v[k], range).text(names[k]));
                track(p, gpu, &r, "Levels", |g| {
                    if let Some(ms) = ops::mask_slot_mut(g, l, i) {
                        ms.source = MaskSource::Levels {
                            in_black: v[0],
                            in_white: v[1],
                            gamma: v[2],
                            out_black: v[3],
                            out_white: v[4],
                        };
                    }
                });
            }
        }
        MaskSource::Node(node) => {
            let n = p.graph.node(node).clone();
            if n.ty == "paint_mask" {
                ui.label(RichText::new("Paint with the Paint tool (B).").weak());
            }
            for def in &n.def().params {
                param_widget(ui, gpu, p, node, def, def.name);
            }
        }
    }
}

fn pretty(name: &str) -> String {
    let mut s = name.replace('_', " ");
    if let Some(f) = s.get(0..1) {
        s.replace_range(0..1, &f.to_uppercase());
    }
    s
}

/// Label + value editor for one node parameter, with an Expose toggle.
pub fn param_widget(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, def: &ParamDef, label: &str) {
    ui.horizontal(|ui| {
        let exposed = p.graph.node(node).exposed.contains(def.name);
        let pin = ui
            .add(egui::Button::new(RichText::new("•").color(if exposed { Color32::from_rgb(240, 190, 60) } else { Color32::from_gray(80) })).small().frame(false))
            .on_hover_text(if exposed { "Exposed on the layer. Click to unexpose." } else { "Expose on the layer" });
        if pin.clicked() {
            p.edit(gpu, "Expose parameter", |g| {
                let e = &mut g.node_mut(node).exposed;
                if !e.remove(def.name) {
                    e.insert(def.name.to_string());
                }
            });
        }
        ui.label(pretty(label));
        param_value(ui, gpu, p, node, def, label);
    });
}

/// Value editor for one parameter.
pub fn param_value(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, def: &ParamDef, id: &str) {
    let v = p.graph.node(node).param(def.name);
    let name = def.name;
    match def.kind {
        ParamKind::Float { min, max } => {
            let mut x = v.as_f32();
            let r = if max - min <= 2.0 {
                ui.add(egui::Slider::new(&mut x, min..=max))
            } else {
                ui.add(egui::DragValue::new(&mut x).range(min..=max).speed((max - min) * 0.002))
            };
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Float(x)));
        }
        ParamKind::Int { min, max } => {
            let mut x = v.as_f32() as i32;
            let r = ui.add(egui::DragValue::new(&mut x).range(min..=max));
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Float(x as f32)));
        }
        ParamKind::Bool => {
            let mut b = v.as_bool();
            let r = ui.checkbox(&mut b, "");
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Bool(b)));
        }
        ParamKind::Color => {
            let mut c = v.as_vec3();
            let r = ui.color_edit_button_rgb(&mut c);
            track(p, gpu, &r, "Edit color", |g| g.set(node, name, Value::Vec3(c)));
        }
        ParamKind::Vec3 => {
            let mut c = v.as_vec3();
            let mut changed = false;
            let mut started = false;
            for k in 0..3 {
                let r = ui.add(egui::DragValue::new(&mut c[k]).speed(0.01));
                changed |= r.changed();
                started |= r.drag_started();
            }
            if started {
                p.checkpoint("Edit parameter");
            }
            if changed {
                p.graph.set(node, name, Value::Vec3(c));
                p.graph_changed(gpu);
            }
        }
        ParamKind::Enum(options) => {
            let mut cur = v.as_str().to_string();
            let mut changed = false;
            egui::ComboBox::from_id_salt((node, id)).selected_text(&cur).show_ui(ui, |ui| {
                for o in options {
                    changed |= ui.selectable_value(&mut cur, o.to_string(), *o).changed();
                }
            });
            if changed {
                p.edit(gpu, "Edit parameter", |g| g.set(node, name, Value::Text(cur)));
            }
        }
        ParamKind::Image => {
            let cur = v.as_str().to_string();
            let label = if cur.is_empty() { "(none)".to_string() } else { cur.clone() };
            if ui.button(label).on_hover_text("Choose an image").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"])
                    .pick_file()
                {
                    match p.import_image(gpu, &path) {
                        Ok(img) => p.edit(gpu, "Set image", |g| g.set(node, name, Value::Text(img))),
                        Err(e) => log::error!("{e:#}"),
                    }
                }
            }
        }
        ParamKind::Stops | ParamKind::Colors => {
            ui.label(RichText::new("Edit in the Node Editor").weak());
        }
    }
}
