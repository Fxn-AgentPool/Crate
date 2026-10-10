//! Library panel: project resources (baked mesh maps, images) and the user library
//! (smart materials, plus categories that are not populated yet).

use std::path::PathBuf;

use crate::gpu::Gpu;
use crate::meshmaps::{MapStatus, MeshMap};
use crate::project::Project;

const THUMB: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    MeshMaps,
    Images,
    SmartMaterials,
    Materials,
    SmartMasks,
    Brushes,
    Alphas,
    Stencils,
    Environments,
}

impl Category {
    const PROJECT: [Category; 2] = [Category::MeshMaps, Category::Images];
    const LIBRARY: [Category; 7] = [
        Category::SmartMaterials,
        Category::Materials,
        Category::SmartMasks,
        Category::Brushes,
        Category::Alphas,
        Category::Stencils,
        Category::Environments,
    ];

    fn name(self) -> &'static str {
        match self {
            Category::MeshMaps => "Mesh Maps",
            Category::Images => "Images",
            Category::SmartMaterials => "Smart Materials",
            Category::Materials => "Materials",
            Category::SmartMasks => "Smart Masks",
            Category::Brushes => "Brushes",
            Category::Alphas => "Alphas",
            Category::Stencils => "Stencils",
            Category::Environments => "Environments",
        }
    }
}

/// What the user asked the library to do this frame.
pub enum LibraryAction {
    InsertSmartMaterial(PathBuf),
    /// Add the map as a mask effect of the selected layer.
    MeshMapMask(MeshMap),
    /// Show the map in the viewport.
    ShowMeshMap(MeshMap),
}

pub struct Library {
    category: Category,
    smart: Option<Vec<(crate::smartmat::LibraryItem, Option<egui::TextureHandle>)>>,
    /// Mesh map thumbnails with the maps version they were made from.
    map_thumbs: [Option<(u64, egui::TextureHandle)>; 6],
}

impl Default for Library {
    fn default() -> Self {
        Self { category: Category::MeshMaps, smart: None, map_thumbs: Default::default() }
    }
}

impl Library {
    pub fn refresh(&mut self) {
        self.smart = None;
    }

    /// Forgets per-project state (thumbnails) when another project is opened.
    pub fn project_changed(&mut self) {
        self.map_thumbs = Default::default();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &Project) -> Option<LibraryAction> {
        let mut chip_row = |ui: &mut egui::Ui, label: &str, cats: &[Category]| {
            ui.label(egui::RichText::new(label).small().weak());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for &c in cats {
                    ui.selectable_value(&mut self.category, c, egui::RichText::new(c.name()).small());
                }
            });
        };
        chip_row(ui, "PROJECT", &Category::PROJECT);
        chip_row(ui, "LIBRARY", &Category::LIBRARY);
        ui.separator();
        match self.category {
            Category::MeshMaps => self.mesh_maps(ui, gpu, p),
            Category::Images => {
                images(ui, p);
                None
            }
            Category::SmartMaterials => self.smart_materials(ui),
            _ => {
                ui.label(egui::RichText::new("Nothing here yet.").weak());
                None
            }
        }
    }

    fn mesh_maps(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &Project) -> Option<LibraryAction> {
        let mut action = None;
        let mut read_one = false;
        let has_layer = p.selection.is_some();
        ui.horizontal_wrapped(|ui| {
            for m in MeshMap::ALL {
                let status = p.maps.status(m);
                // Thumbnails are read back from the GPU, at most one per frame.
                let stale = self.map_thumbs[m.index()].as_ref().is_none_or(|(v, _)| *v != p.maps.version);
                if status != MapStatus::Missing && stale && !read_one {
                    read_one = true;
                    let img = map_thumbnail(gpu, p, m);
                    let tex = ui.ctx().load_texture(format!("meshmap {}", m.name()), img, egui::TextureOptions::LINEAR);
                    self.map_thumbs[m.index()] = Some((p.maps.version, tex));
                }
                if read_one && stale {
                    ui.ctx().request_repaint();
                }
                let id = egui::Id::new(("meshmap", m.index()));
                let resp = ui
                    .dnd_drag_source(id, m, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(72.0);
                            let size = egui::vec2(72.0, 72.0);
                            match (&self.map_thumbs[m.index()], status) {
                                (Some((_, t)), s) if s != MapStatus::Missing => {
                                    ui.add(egui::Image::new(t).fit_to_exact_size(size).corner_radius(3.0));
                                }
                                _ => {
                                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                                    ui.painter().rect_filled(r, 3.0, egui::Color32::from_gray(30));
                                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "not baked", egui::FontId::proportional(10.0), egui::Color32::from_gray(110));
                                }
                            }
                            let mut name = egui::RichText::new(m.name()).small();
                            if status == MapStatus::OutOfDate {
                                name = name.color(egui::Color32::from_rgb(230, 170, 60));
                            }
                            ui.label(name);
                        });
                    })
                    .response;
                let hint = match status {
                    MapStatus::Missing => "Not baked yet: bake it in Window > Mesh Maps.\n",
                    MapStatus::OutOfDate => "Out of date: rebake to match the current settings.\n",
                    MapStatus::UpToDate => "",
                };
                let resp = resp.on_hover_text(format!(
                    "{hint}Double-click: add as a mask effect of the selected layer\nDrag onto the Node Editor: add a Mesh Map node"
                ));
                if resp.double_clicked() && has_layer {
                    action = Some(LibraryAction::MeshMapMask(m));
                }
                resp.context_menu(|ui| {
                    if ui.add_enabled(has_layer, egui::Button::new("Add to mask of selected layer")).clicked() {
                        action = Some(LibraryAction::MeshMapMask(m));
                        ui.close();
                    }
                    if ui.button("Show in viewport").clicked() {
                        action = Some(LibraryAction::ShowMeshMap(m));
                        ui.close();
                    }
                });
            }
        });
        action
    }

    fn smart_materials(&mut self, ui: &mut egui::Ui) -> Option<LibraryAction> {
        let items = self.smart.get_or_insert_with(|| {
            crate::smartmat::list()
                .into_iter()
                .map(|it| {
                    let tex = it.thumbnail.clone().map(|img| ui.ctx().load_texture(format!("smat {}", it.name), img, Default::default()));
                    (it, tex)
                })
                .collect()
        });
        let mut chosen = None;
        if items.is_empty() {
            ui.label(egui::RichText::new("None yet. Right-click a group > Save as smart material.").weak());
        }
        ui.horizontal_wrapped(|ui| {
            for (it, tex) in items.iter() {
                let id = egui::Id::new(("smat", &it.path));
                let resp = ui
                    .dnd_drag_source(id, it.path.clone(), |ui| {
                        ui.vertical(|ui| {
                            if let Some(t) = tex {
                                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(72.0, 72.0)));
                            }
                            ui.label(egui::RichText::new(&it.name).small());
                        });
                    })
                    .response
                    .on_hover_text("Drag onto the Layers panel, or double-click to add");
                if resp.double_clicked() {
                    chosen = Some(it.path.clone());
                }
                resp.context_menu(|ui| {
                    if ui.button("Add to layers").clicked() {
                        chosen = Some(it.path.clone());
                        ui.close();
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button("Refresh").clicked() {
                self.smart = None;
            }
            if ui.small_button("Show in file browser").clicked() {
                let dir = crate::smartmat::library_dir();
                let _ = std::fs::create_dir_all(&dir);
                crate::prefs::open_folder(&dir);
            }
        });
        chosen.map(LibraryAction::InsertSmartMaterial)
    }
}

fn images(ui: &mut egui::Ui, p: &Project) {
    if p.image_sizes.is_empty() {
        ui.label(egui::RichText::new("No images imported.").weak());
        return;
    }
    for (name, (w, h)) in &p.image_sizes {
        let r = ui.label(format!("{name}  ({w}×{h})"));
        if let Some(src) = p.image_sources.get(name) {
            r.on_hover_text(src.display().to_string());
        }
    }
}

/// Reads the first tile of a mesh map back and shrinks it to a thumbnail.
fn map_thumbnail(gpu: &Gpu, p: &Project, m: MeshMap) -> egui::ColorImage {
    let maps = &p.maps;
    let (tex, bpp) = match m {
        MeshMap::Position => (&maps.position.texture, 16),
        MeshMap::WorldNormal => (&maps.normal.texture, 8),
        MeshMap::Id => (&maps.id.texture, 4),
        MeshMap::AmbientOcclusion => (&maps.ao.texture, 4),
        MeshMap::Curvature => (&maps.curvature.texture, 4),
        MeshMap::Thickness => (&maps.thickness.texture, 4),
    };
    let bytes = gpu.read_texture(tex, 0, bpp);
    let res = maps.res as usize;
    let texel = |i: usize| -> [f32; 3] {
        let o = i * bpp as usize;
        let f = |k: usize| f32::from_le_bytes(bytes[o + k * 4..o + k * 4 + 4].try_into().unwrap());
        let h = |k: usize| half::f16::from_le_bytes([bytes[o + k * 2], bytes[o + k * 2 + 1]]).to_f32();
        match m {
            MeshMap::Position => [f(0), f(1), f(2)],
            // Shown the way normal maps usually look: -1..1 mapped to 0..1.
            MeshMap::WorldNormal => [h(0) * 0.5 + 0.5, h(1) * 0.5 + 0.5, h(2) * 0.5 + 0.5],
            MeshMap::Id => [bytes[o] as f32 / 255.0, bytes[o + 1] as f32 / 255.0, bytes[o + 2] as f32 / 255.0],
            _ => [f(0); 3],
        }
    };
    // Box filter (sparsely sampled) down to THUMB².
    let step = (res / THUMB).max(1);
    let mut px = Vec::with_capacity(THUMB * THUMB * 3);
    for ty in 0..THUMB {
        for tx in 0..THUMB {
            let mut sum = [0.0f32; 3];
            let mut n = 0.0f32;
            let (x0, y0) = (tx * res / THUMB, ty * res / THUMB);
            for y in (y0..(y0 + step).min(res)).step_by((step / 4).max(1)) {
                for x in (x0..(x0 + step).min(res)).step_by((step / 4).max(1)) {
                    let c = texel(y * res + x);
                    sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]];
                    n += 1.0;
                }
            }
            for c in sum {
                px.push(((c / n.max(1.0)).clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
        }
    }
    egui::ColorImage::from_rgb([THUMB, THUMB], &px)
}
