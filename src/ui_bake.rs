//! The Bake Exclude tool: its selection, its display in the viewport, and its toolbar actions.
//! See docs/reference/bake-exclusion.md.

use std::collections::BTreeSet;

use egui::RichText;
use wgpu::util::DeviceExt;

use crate::geo::{Element, Stacks};
use crate::gpu::Gpu;
use crate::project::Project;
use crate::ui_theme as t;
use crate::ui_widgets as w;
use crate::viewport::{TintEdge, TintVertex};

/// The display's GPU buffers, made again when the exclusion, selection or mesh changes.
#[derive(Default)]
pub struct BakeView {
    key: Option<(u64, u64)>,
    tris: Option<(wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
}

impl BakeView {
    /// Stacks tinted in their colors, excluded triangles outside stacks darkened, the selection
    /// tinted white, and bake sources outlined in their stack's color
    /// (docs/reference/bake-exclusion.md#display).
    pub fn update(&mut self, gpu: &Gpu, p: &Project) {
        let key = (p.bake_version, p.mesh_version());
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let (mesh, geo) = (&p.mesh, p.geo());
        let stacks = geo.stacks(mesh);
        let vertex = |i: u32, color: [f32; 4]| {
            let v = &mesh.vertices[i as usize];
            TintVertex { pos: v.pos, uv: v.uv, color }
        };
        let stack_of = |t: u32| stacks.of_island.get(&geo.island(t)).copied();
        let source_islands: BTreeSet<u32> = p.bake_mask.sources.iter().map(|s| geo.island(*s)).collect();
        let mut tris = Vec::new();
        for (t, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            let t = t as u32;
            let color = if p.bake_sel.contains(&t) {
                [1.0, 1.0, 1.0, 0.4]
            } else if let Some(s) = stack_of(t) {
                // The source lighter, so its outline stands out.
                let [r, g, b] = Stacks::color(s);
                [r, g, b, if source_islands.contains(&geo.island(t)) { 0.2 } else { 0.5 }]
            } else if p.bake_mask.is_excluded(t) {
                [0.0, 0.0, 0.0, 0.6]
            } else {
                continue;
            };
            tris.extend(tri.map(|i| vertex(i, color)));
        }
        let mut edges = Vec::new();
        for &source in &p.bake_mask.sources {
            let [r, g, b] = stack_of(source).map_or([1.0; 3], Stacks::color);
            for e in geo.island_outline(mesh, geo.island(source)).chunks(2) {
                let (a, z) = (&mesh.vertices[e[0] as usize], &mesh.vertices[e[1] as usize]);
                edges.push(TintEdge { a_pos: a.pos, a_uv: a.uv, b_pos: z.pos, b_uv: z.uv, color: [r, g, b, 1.0] });
            }
        }
        fn buffer<T: bytemuck::Pod>(gpu: &Gpu, label: &str, v: &[T]) -> Option<(wgpu::Buffer, u32)> {
            (!v.is_empty()).then(|| {
                let b = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(v), usage: wgpu::BufferUsages::VERTEX });
                (b, v.len() as u32)
            })
        }
        self.tris = buffer(gpu, "bake exclusion tint", &tris);
        self.lines = buffer(gpu, "bake exclusion outlines", &edges);
    }

    /// The display, added to Geo Fill's outlines and highlight.
    pub fn add_to<'a>(&'a self, overlay: &mut crate::viewport::Overlay<'a>) {
        overlay.tint_tris = self.tris.as_ref().map(|(b, n)| (b, *n));
        overlay.tint_lines = self.lines.as_ref().map(|(b, n)| (b, *n));
    }
}

/// Replaces the selection with `elements`' triangles, or adds them.
pub fn select(p: &mut Project, elements: &BTreeSet<Element>, add: bool) {
    let tris = p.geo().triangles_of(&p.mesh, elements);
    if !add {
        p.bake_sel.clear();
    }
    p.bake_sel.extend(tris);
    p.bake_version += 1;
}

pub fn clear_selection(p: &mut Project) {
    if !p.bake_sel.is_empty() {
        p.bake_sel.clear();
        p.bake_version += 1;
    }
}

/// Exclude or Include the selection, one undo step.
pub fn set_excluded(gpu: &Gpu, p: &mut Project, on: bool) -> Result<(), String> {
    if p.bake_sel.is_empty() {
        return Err("Nothing selected.".into());
    }
    let tris: Vec<u32> = p.bake_sel.iter().copied().collect();
    let mut mask = p.bake_mask.clone();
    mask.set(&p.mesh, p.geo(), &tris, on);
    p.set_bake_mask(gpu, if on { "Exclude from baking" } else { "Include in baking" }, mask)
}

/// Make bake source, one undo step.
pub fn make_sources(gpu: &Gpu, p: &mut Project) -> Result<(), String> {
    let mut mask = p.bake_mask.clone();
    if mask.make_sources(&p.mesh, p.geo(), &p.bake_sel) == 0 {
        return Err("Select an island that is stacked with others.".into());
    }
    p.set_bake_mask(gpu, "Make bake source", mask)
}

/// Clear: every triangle included, no bake sources.
pub fn clear(gpu: &Gpu, p: &mut Project) -> Result<(), String> {
    p.set_bake_mask(gpu, "Clear bake exclusion", Default::default())
}

/// The toolbar's actions after Select and Fill by. Returns a message for the status bar.
pub fn toolbar_actions(ui: &mut egui::Ui, gpu: &Gpu, p: &mut Project) -> Option<String> {
    let mut result = Ok(());
    let has_sel = !p.bake_sel.is_empty();
    if ui.add_enabled(has_sel, w::secondary("Exclude")).on_hover_text("Leave the selected geometry out of mesh map baking").clicked() {
        result = set_excluded(gpu, p, true);
    }
    if ui.add_enabled(has_sel, w::secondary("Include")).on_hover_text("Bake the selected geometry again").clicked() {
        result = set_excluded(gpu, p, false);
    }
    if ui.add_enabled(has_sel, w::secondary("Make bake source")).on_hover_text("Bake the selected copies' stacks from them; exclude the other copies").clicked() {
        result = make_sources(gpu, p);
    }
    ui.label(RichText::new(format!("{} excluded", p.bake_mask.count())).color(t::TEXT_2));
    result.err()
}
