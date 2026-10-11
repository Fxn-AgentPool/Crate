//! Layers and Properties panels. See docs/reference/layers.md and windows-and-panels.md.

use egui::{Color32, RichText};

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::graph::{self, BlendMode, Graph, MaskSource, NodeId, ParamDef, ParamKind, Value};
use crate::ops::{self, LayerKind, LayerRef, Selection};
use crate::project::Project;
use crate::ui_theme as t;
use crate::ui_widgets::{Chip, Field, Section, Toggle};

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

// ------------------------------------------------------------------ properties

/// Name of the selected layer or mask layer, for the Properties header.
pub fn selection_name(p: &Project) -> Option<String> {
    match p.selection? {
        Selection::Layer(l) => ops::slot(&p.graph, l).map(|s| s.name.clone()),
        Selection::MaskLayer(l, i) => ops::mask_slot(&p.graph, l, i).map(|m| m.name.clone()),
    }
}

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
pub fn track(p: &mut Project, gpu: &Gpu, resp: &egui::Response, label: &str, apply: impl FnOnce(&mut Graph)) {
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
    let channels: Vec<Channel> = Channel::ALL.into_iter().filter(|c| p.channels.is_enabled(*c)).collect();

    Section::new("Channels").id_salt("layer channels").show(ui, |ui| {
        let gap = ui.spacing().item_spacing.x * 0.5;
        let avail = ui.available_width() - 2.0 * gap;
        let name_w = (avail * 0.28).floor();
        let combo_w = (avail * 0.32).floor();
        let field_w = avail - name_w - combo_w;
        for c in channels.iter().copied() {
            let cb = slot.channels[c.index()];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let mut enabled = cb.enabled;
                let r = ui.add(Chip::new(&mut enabled, c.name(), name_w));
                track(p, gpu, &r, "Layer channel", |g| ops::set_channel_enabled(g, l, c, enabled));
                ui.add_enabled_ui(cb.enabled, |ui| {
                    let mut mode = cb.mode;
                    let mut changed = false;
                    // Amber when the mode isn't Normal, so non-default blends stand out.
                    let text = if mode == BlendMode::Normal { RichText::new(mode.name()) } else { RichText::new(mode.name()).color(t::ATTENTION) };
                    // A fixed column: long mode names are cut rather than pushing the field over.
                    ui.allocate_ui(egui::vec2(combo_w, t::H_CONTROL), |ui| {
                        egui::ComboBox::from_id_salt(("mode", c.index())).width(combo_w).truncate().selected_text(text).show_ui(ui, |ui| {
                            for m in BlendMode::for_channel(c) {
                                changed |= ui.selectable_value(&mut mode, *m, m.name()).changed();
                            }
                        });
                    });
                    if changed {
                        p.edit(gpu, "Blend mode", |g| {
                            if let Some(s) = ops::slot_mut(g, l) {
                                s.channels[c.index()].mode = mode;
                            }
                        });
                    }
                    let mut op = cb.opacity * 100.0;
                    let field_w = ui.available_width().min(field_w);
                    let r = ui.add(Field::new(&mut op, 0.0..=100.0).decimals(0).width(field_w)).on_hover_text("Channel opacity (%)");
                    track(p, gpu, &r, "Channel opacity", |g| {
                        if let Some(s) = ops::slot_mut(g, l) {
                            s.channels[c.index()].opacity = op / 100.0;
                        }
                    });
                });
            });
        }
    });

    let Some(content) = slot.content else { return };
    match kind {
        LayerKind::Fill => {
            let on: Vec<Channel> = channels.into_iter().filter(|c| slot.channels[c.index()].enabled).collect();
            fill_properties(ui, gpu, p, content, &on);
        }
        LayerKind::Paint => {
            crate::ui_widgets::hint(ui, "Paint with the Paint tool (B).");
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
    Section::new("Exposed parameters").id_salt("layer exposed parameters").show(ui, |ui| {
        for n in nodes {
            let node = p.graph.node(n).clone();
            for def in &node.def().params {
                if node.exposed.contains(def.name) {
                    param_widget(ui, gpu, p, n, def, &format!("{}: {}", node.title(), def.name));
                }
            }
        }
    });
}

/// Narrowest X, Y or Z field of a Fill value before the three move to a line of their own.
const VEC3_FIELD_MIN: f32 = 48.0;

/// The Projection and Values sections of a Fill layer; `channels` are the ones the layer affects.
fn fill_properties(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, channels: &[Channel]) {
    let n = p.graph.node(node).clone();
    let defs = &n.def().params;
    let find = |name: &str| defs.iter().find(|d| d.name == name).unwrap();
    let triplanar = n.param("projection").as_str() == "Triplanar";

    Section::new("Projection").id_salt("fill projection").show(ui, |ui| {
        let rows = [("projection", "Type"), ("scale", "Scale"), ("offset", "Offset"), ("rotation", "Rotation"), ("triplanar_blend", "Triplanar blend")];
        for (name, label) in rows {
            if name == "triplanar_blend" && !triplanar {
                continue;
            }
            let def = find(name);
            ui.horizontal(|ui| {
                expose_pin(ui, gpu, p, node, def.name);
                crate::ui_widgets::setting(ui, label, |ui| param_value(ui, gpu, p, node, def, def.name));
            });
        }
    });

    Section::new("Values").id_salt("fill values").show(ui, |ui| {
        if channels.is_empty() {
            crate::ui_widgets::hint(ui, "Turn on a channel in Channels.");
        }
        let source_w = ((ui.available_width() - crate::ui_widgets::LABEL_WIDTH) * 0.42).floor();
        for c in channels.iter().copied() {
            let (on, val, src, img) = graph::fill_param_names(c);
            // Greyed out when the Fill node's own output was turned off elsewhere, e.g. in the Node Editor.
            let value = if n.param(src).as_str() == "Image" { img } else { val };
            ui.add_enabled_ui(n.param(on).as_bool(), |ui| {
                // X, Y, Z that don't fit before the source go on a line of their own below it.
                let below = std::cell::Cell::new(false);
                crate::ui_widgets::setting(ui, c.name(), |ui| {
                    // The source sits at the right end; the value fills the space before it.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.allocate_ui(egui::vec2(source_w, t::H_CONTROL), |ui| {
                            ui.spacing_mut().combo_width = source_w;
                            param_value(ui, gpu, p, node, find(src), src);
                        });
                        let value_w = ui.available_width().max(40.0);
                        if matches!(find(value).kind, ParamKind::Vec3) && value_w < 3.0 * VEC3_FIELD_MIN + 2.0 * ui.spacing().item_spacing.x {
                            below.set(true);
                            return;
                        }
                        ui.allocate_ui_with_layout(egui::vec2(value_w, t::H_CONTROL), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            // The color swatch takes the interact size: give it the whole column.
                            ui.spacing_mut().interact_size = egui::vec2(value_w, t::H_CONTROL);
                            param_value(ui, gpu, p, node, find(value), value);
                        });
                    });
                });
                if below.get() {
                    ui.horizontal(|ui| {
                        ui.add_space(crate::ui_widgets::LABEL_WIDTH);
                        param_value(ui, gpu, p, node, find(value), value);
                    });
                }
            });
        }
    });
}

fn mask_layer_properties(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, l: LayerRef, i: usize) {
    let Some(m) = ops::mask_slot(&p.graph, l, i).cloned() else { return };
    let mut opacity = m.opacity * 100.0;
    let r = ui.add(Field::new(&mut opacity, 0.0..=100.0).label("Opacity").decimals(0).suffix(" %"));
    track(p, gpu, &r, "Mask opacity", |g| {
        if let Some(ms) = ops::mask_slot_mut(g, l, i) {
            ms.opacity = opacity / 100.0;
        }
    });
    let mut mode = m.mode;
    let mut changed = false;
    crate::ui_widgets::setting(ui, "Blend mode", |ui| {
        egui::ComboBox::from_id_salt("mask blend mode").width(ui.available_width()).selected_text(mode.name()).show_ui(ui, |ui| {
            for b in BlendMode::MASK {
                changed |= ui.selectable_value(&mut mode, *b, b.name()).changed();
            }
        });
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
                let r = ui.add(Field::new(&mut v[k], range).label(names[k]));
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
        expose_pin(ui, gpu, p, node, def.name);
        match def.kind {
            // Numbers carry their label inside the field.
            ParamKind::Float { .. } | ParamKind::Int { .. } => number_value(ui, gpu, p, node, def, &pretty(label)),
            _ => {
                crate::ui_widgets::setting(ui, &pretty(label), |ui| param_value(ui, gpu, p, node, def, label));
            }
        }
    });
}

/// A Float or Int parameter as a field labelled `label`.
fn number_value(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, def: &ParamDef, label: &str) {
    let v = p.graph.node(node).param(def.name);
    let name = def.name;
    match def.kind {
        ParamKind::Float { min, max } => {
            let mut x = v.as_f32();
            let field = Field::new(&mut x, min..=max).label(label);
            // Wide ranges are limits rather than scales: no fill, a fixed step.
            let field = if max - min <= 2.0 { field } else { field.no_fill().speed(((max - min) * 0.002) as f64) };
            let r = ui.add(field);
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Float(x)));
        }
        ParamKind::Int { min, max } => {
            let mut x = v.as_f32() as i32;
            let r = ui.add(Field::new(&mut x, min..=max).label(label).no_fill().speed(0.1));
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Float(x as f32)));
        }
        _ => {}
    }
}

/// Toggles whether a node parameter is exposed on its layer.
pub fn expose_pin(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, name: &str) {
    let exposed = p.graph.node(node).exposed.contains(name);
    let pin = ui
        .add(egui::Button::new(RichText::new("•").color(if exposed { Color32::from_rgb(240, 190, 60) } else { Color32::from_gray(80) })).small().frame(false))
        .on_hover_text(if exposed { "Exposed on the layer. Click to unexpose." } else { "Expose on the layer" });
    if pin.clicked() {
        p.edit(gpu, "Expose parameter", |g| {
            let e = &mut g.node_mut(node).exposed;
            if !e.remove(name) {
                e.insert(name.to_string());
            }
        });
    }
}

/// Value editor for one parameter.
pub fn param_value(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project, node: NodeId, def: &ParamDef, id: &str) {
    let v = p.graph.node(node).param(def.name);
    let name = def.name;
    match def.kind {
        ParamKind::Float { min, max } => {
            let mut x = v.as_f32();
            let field = Field::new(&mut x, min..=max);
            let field = if max - min <= 2.0 { field } else { field.no_fill().speed(((max - min) * 0.002) as f64) };
            let r = ui.add(field);
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Float(x)));
        }
        ParamKind::Int { min, max } => {
            let mut x = v.as_f32() as i32;
            let r = ui.add(Field::new(&mut x, min..=max).no_fill().speed(0.1));
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Float(x as f32)));
        }
        ParamKind::Bool => {
            let mut b = v.as_bool();
            let r = ui.add(Toggle(&mut b));
            track(p, gpu, &r, "Edit parameter", |g| g.set(node, name, Value::Bool(b)));
        }
        ParamKind::Color => {
            let mut c = v.as_vec3();
            let r = crate::ui_widgets::color_edit(ui, &mut c);
            track(p, gpu, &r, "Edit color", |g| g.set(node, name, Value::Vec3(c)));
        }
        ParamKind::Vec3 => {
            let mut c = v.as_vec3();
            let mut changed = false;
            let mut started = false;
            let fw = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(40.0);
            for v in &mut c {
                let r = ui.add(Field::new(v, -1.0e6..=1.0e6).decimals(2).speed(0.01).no_fill().width(fw));
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
            if ui.add(crate::ui_widgets::secondary(label)).on_hover_text("Choose an image").clicked() {
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
        ParamKind::Colors => {
            ui.vertical(|ui| crate::ui_graph::id_editor(ui, gpu, p, node));
        }
        ParamKind::Stops => {
            ui.label(RichText::new("Edit in the Node Editor").weak());
        }
    }
}
