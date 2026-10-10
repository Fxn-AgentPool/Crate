//! Library panel: project resources (baked mesh maps, images) and the user library, one
//! category per library folder. See docs/reference/windows-and-panels.md "Library".

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

use crate::gpu::Gpu;
use crate::library::{self, Kind};
use crate::meshmaps::{MapStatus, MeshMap};
use crate::project::Project;

const THUMB: usize = 64;
/// Size library image thumbnails are made at (shown at 72 points).
const LIBRARY_THUMB: u32 = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Category {
    MeshMaps,
    Images,
    /// A folder of the user library.
    Library(String),
}

/// What the user asked the library to do this frame.
pub enum LibraryAction {
    InsertSmartMaterial(PathBuf),
    /// Add the map as a mask effect of the selected layer.
    MeshMapMask(MeshMap),
    /// Show the map in the viewport.
    ShowMeshMap(MeshMap),
    UseAlpha(PathBuf),
    LoadStencil(PathBuf),
    LoadEnvironment(PathBuf),
}

enum Thumb {
    Loading,
    Ready(egui::TextureHandle),
    Unreadable,
}

/// A scanned library folder, with thumbnails arriving from a background thread.
struct FolderView {
    items: Vec<(library::Item, Thumb)>,
    thumbs: Option<mpsc::Receiver<(usize, Option<egui::ColorImage>)>>,
}

pub struct Library {
    category: Category,
    smart: Option<Vec<(crate::smartmat::LibraryItem, Option<egui::TextureHandle>)>>,
    /// Folders the user added, scanned when first shown and on Refresh.
    extra: Option<Vec<String>>,
    folders: HashMap<String, FolderView>,
    /// Mesh map thumbnails with the maps version they were made from.
    map_thumbs: [Option<(u64, egui::TextureHandle)>; 8],
}

impl Default for Library {
    fn default() -> Self {
        Self { category: Category::MeshMaps, smart: None, extra: None, folders: HashMap::new(), map_thumbs: Default::default() }
    }
}

impl Library {
    /// Rescans every library folder (Refresh, or after saving a smart material).
    pub fn refresh(&mut self) {
        self.smart = None;
        self.extra = None;
        // Dropping a view also stops its thumbnail thread.
        self.folders.clear();
    }

    /// Forgets per-project state (thumbnails) when another project is opened.
    pub fn project_changed(&mut self) {
        self.map_thumbs = Default::default();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, gpu: &Gpu, p: &Project) -> Option<LibraryAction> {
        let extra = self.extra.get_or_insert_with(library::extra_folders).clone();
        if let Category::Library(f) = &self.category {
            if library::kind(f) == Kind::Images && !extra.contains(f) {
                // The folder was removed.
                self.category = Category::Library(library::BUILT_IN[0].0.into());
            }
        }
        let mut chip_row = |ui: &mut egui::Ui, label: &str, cats: Vec<(Category, String)>| {
            ui.label(egui::RichText::new(label).small().weak());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for (c, name) in cats {
                    ui.selectable_value(&mut self.category, c, egui::RichText::new(name).small());
                }
            });
        };
        chip_row(ui, "PROJECT", vec![(Category::MeshMaps, "Mesh Maps".into()), (Category::Images, "Images".into())]);
        let folders = library::BUILT_IN.iter().map(|b| b.0.to_string()).chain(extra);
        chip_row(ui, "LIBRARY", folders.map(|f| (Category::Library(f.clone()), library::title(&f))).collect());
        ui.separator();
        match self.category.clone() {
            Category::MeshMaps => self.mesh_maps(ui, gpu, p),
            Category::Images => {
                images(ui, p);
                None
            }
            Category::Library(folder) => {
                let action = if library::kind(&folder) == Kind::SmartMaterials { self.smart_materials(ui) } else { self.folder(ui, &folder) };
                ui.horizontal(|ui| {
                    if ui.small_button("Refresh").clicked() {
                        self.refresh();
                    }
                    if ui.small_button("Show in file browser").clicked() {
                        let dir = library::dir(&folder);
                        let _ = std::fs::create_dir_all(&dir);
                        crate::prefs::open_folder(&dir);
                    }
                });
                action
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
        chosen.map(LibraryAction::InsertSmartMaterial)
    }

    /// Any library folder other than smart materials.
    fn folder(&mut self, ui: &mut egui::Ui, folder: &str) -> Option<LibraryAction> {
        let kind = library::kind(folder);
        let thumbnails = matches!(kind, Kind::Alphas | Kind::Stencils | Kind::Images);
        let view = self.folders.entry(folder.to_string()).or_insert_with(|| {
            let items = library::scan(folder);
            let thumbs = thumbnails.then(|| spawn_thumbnails(ui.ctx(), items.iter().map(|it| it.path.clone()).collect()));
            FolderView { items: items.into_iter().map(|it| (it, Thumb::Loading)).collect(), thumbs }
        });
        if let Some(rx) = &view.thumbs {
            loop {
                match rx.try_recv() {
                    Ok((i, img)) => {
                        view.items[i].1 = match img {
                            Some(img) => {
                                let name = format!("library {}", view.items[i].0.path.display());
                                Thumb::Ready(ui.ctx().load_texture(name, img, egui::TextureOptions::LINEAR))
                            }
                            None => Thumb::Unreadable,
                        };
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        view.thumbs = None;
                        break;
                    }
                }
            }
        }
        if view.items.is_empty() {
            ui.label(egui::RichText::new("Empty. Add files to the folder (Show in file browser), then Refresh.").weak());
            return None;
        }
        // What double-click and the context menu do; None lists the files without actions.
        let use_item: Option<(&str, fn(PathBuf) -> LibraryAction)> = match kind {
            Kind::Alphas => Some(("Use as brush alpha", LibraryAction::UseAlpha)),
            Kind::Stencils | Kind::Images => Some(("Load as stencil", LibraryAction::LoadStencil)),
            Kind::Environments => Some(("Load environment", LibraryAction::LoadEnvironment)),
            Kind::SmartMaterials | Kind::Reserved => None,
        };
        let mut action = None;
        let mut item_ui = |resp: egui::Response, it: &library::Item| {
            let file = it.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let Some((label, make)) = use_item else {
                resp.on_hover_text(file);
                return;
            };
            let resp = resp.on_hover_text(format!("{file}\nDouble-click: {}", label.to_lowercase()));
            if resp.double_clicked() {
                action = Some(make(it.path.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button(label).clicked() {
                    action = Some(make(it.path.clone()));
                    ui.close();
                }
            });
        };
        if thumbnails {
            ui.horizontal_wrapped(|ui| {
                for (it, thumb) in &view.items {
                    let resp = ui
                        .vertical(|ui| {
                            ui.set_width(72.0);
                            let size = egui::vec2(72.0, 72.0);
                            match thumb {
                                Thumb::Ready(t) => {
                                    ui.add(egui::Image::new(t).fit_to_exact_size(size).corner_radius(3.0));
                                }
                                Thumb::Loading | Thumb::Unreadable => {
                                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                                    ui.painter().rect_filled(r, 3.0, egui::Color32::from_gray(30));
                                    let text = if matches!(thumb, Thumb::Unreadable) { "unreadable" } else { "…" };
                                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(10.0), egui::Color32::from_gray(110));
                                }
                            }
                            ui.add(egui::Label::new(egui::RichText::new(&it.name).small()).truncate());
                        })
                        .response
                        .interact(egui::Sense::click());
                    item_ui(resp, it);
                }
            });
        } else {
            for (it, _) in &view.items {
                let resp = ui.add(egui::Label::new(&it.name).sense(egui::Sense::click()));
                item_ui(resp, it);
            }
        }
        action
    }
}

/// Makes thumbnails of `paths` in the background; each arrives as (index, image, or None when
/// the file can't be read). The thread stops early when the receiver is dropped.
fn spawn_thumbnails(ctx: &egui::Context, paths: Vec<PathBuf>) -> mpsc::Receiver<(usize, Option<egui::ColorImage>)> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("library thumbnails".into()).spawn(move || {
        for (i, path) in paths.iter().enumerate() {
            let img = image::open(path).ok().map(|img| {
                let t = img.thumbnail(LIBRARY_THUMB, LIBRARY_THUMB).to_rgba8();
                egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
            });
            if tx.send((i, img)).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    });
    if let Err(e) = spawned {
        log::error!("could not start the thumbnail thread: {e}");
    }
    rx
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
        MeshMap::Normal => (&maps.tnormal.texture, 8),
        MeshMap::Height => (&maps.height.texture, 4),
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
            // Stored encoded already.
            MeshMap::Normal => [h(0), h(1), h(2)],
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
