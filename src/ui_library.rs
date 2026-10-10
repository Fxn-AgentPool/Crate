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
/// Tiles: square for images, 2:1 for environments (equirectangular panoramas).
const TILE: egui::Vec2 = egui::vec2(72.0, 72.0);
const WIDE_TILE: egui::Vec2 = egui::vec2(144.0, 72.0);
/// Thumbnails are made at about twice the tile size, for high-DPI screens.
const THUMB_SCALE: f32 = 2.0;

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

/// A list of image files, with thumbnails arriving from a background thread.
struct FolderView {
    items: Vec<(library::Item, Thumb)>,
    thumbs: Option<mpsc::Receiver<(usize, Option<egui::ColorImage>)>>,
}

impl FolderView {
    /// `tile`: thumbnails fitting this tile are made in the background; None lists names only.
    fn new(ctx: &egui::Context, items: Vec<library::Item>, tile: Option<egui::Vec2>) -> Self {
        let thumbs = tile.map(|t| {
            let max = [(t.x * THUMB_SCALE) as u32, (t.y * THUMB_SCALE) as u32];
            spawn_thumbnails(ctx, items.iter().map(|it| it.path.clone()).collect(), max)
        });
        Self { items: items.into_iter().map(|it| (it, Thumb::Loading)).collect(), thumbs }
    }

    /// Takes the thumbnails that are ready.
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.thumbs else { return };
        loop {
            match rx.try_recv() {
                Ok((i, img)) => {
                    self.items[i].1 = match img {
                        Some(img) => {
                            let name = format!("library {}", self.items[i].0.path.display());
                            Thumb::Ready(ctx.load_texture(name, img, egui::TextureOptions::LINEAR))
                        }
                        None => Thumb::Unreadable,
                    };
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.thumbs = None;
                    break;
                }
            }
        }
    }
}

pub struct Library {
    category: Category,
    smart: Option<Vec<(crate::smartmat::LibraryItem, Option<egui::TextureHandle>)>>,
    /// Folders the user added, scanned when first shown and on Refresh.
    extra: Option<Vec<String>>,
    folders: HashMap<String, FolderView>,
    /// Project images (name, source) and their thumbnails; rebuilt when the list changes.
    images: Option<(Vec<(String, PathBuf)>, FolderView)>,
    /// Mesh map thumbnails with the maps version they were made from.
    map_thumbs: [Option<(u64, egui::TextureHandle)>; 8],
}

impl Default for Library {
    fn default() -> Self {
        Self { category: Category::MeshMaps, smart: None, extra: None, folders: HashMap::new(), images: None, map_thumbs: Default::default() }
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
        self.images = None;
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
                self.images(ui, p);
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
        let tile = match kind {
            Kind::Alphas | Kind::Stencils | Kind::Images => Some(TILE),
            Kind::Environments => Some(WIDE_TILE),
            Kind::SmartMaterials | Kind::Reserved => None,
        };
        let view = self.folders.entry(folder.to_string()).or_insert_with(|| FolderView::new(ui.ctx(), library::scan(folder), tile));
        view.poll(ui.ctx());
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
        if let Some(tile) = tile {
            ui.horizontal_wrapped(|ui| {
                for (it, thumb) in &view.items {
                    let resp = thumbnail_tile(ui, thumb, tile, &it.name).interact(egui::Sense::click());
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

/// Makes thumbnails of `paths`, at most `max` pixels, in the background; each arrives as
/// (index, image, or None when the file can't be read). The thread stops early when the
/// receiver is dropped.
fn spawn_thumbnails(ctx: &egui::Context, paths: Vec<PathBuf>, max: [u32; 2]) -> mpsc::Receiver<(usize, Option<egui::ColorImage>)> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("library thumbnails".into()).spawn(move || {
        for (i, path) in paths.iter().enumerate() {
            let img = image::open(path).ok().map(|img| thumbnail(&img, max));
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

impl Library {
    fn images(&mut self, ui: &mut egui::Ui, p: &Project) {
        if p.image_sizes.is_empty() {
            ui.label(egui::RichText::new("No images imported.").weak());
            return;
        }
        // Sorted by name (a BTreeMap); an image without a source shows as unreadable.
        let list: Vec<(String, PathBuf)> =
            p.image_sizes.keys().map(|n| (n.clone(), p.image_sources.get(n).cloned().unwrap_or_default())).collect();
        if self.images.as_ref().is_none_or(|(l, _)| *l != list) {
            let items = list.iter().map(|(name, path)| library::Item { name: name.clone(), path: path.clone() }).collect();
            self.images = Some((list, FolderView::new(ui.ctx(), items, Some(TILE))));
        }
        let Some((_, view)) = &mut self.images else { return };
        view.poll(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            for (it, thumb) in &view.items {
                let (w, h) = p.image_sizes.get(&it.name).copied().unwrap_or_default();
                let source = if it.path.as_os_str().is_empty() { "no source file".to_string() } else { it.path.display().to_string() };
                thumbnail_tile(ui, thumb, TILE, &it.name).on_hover_text(format!("{w}×{h}\n{source}"));
            }
        });
    }
}

/// A thumbnail fitted into `tile` (keeping its aspect ratio) with the name below.
fn thumbnail_tile(ui: &mut egui::Ui, thumb: &Thumb, tile: egui::Vec2, name: &str) -> egui::Response {
    ui.vertical(|ui| {
        ui.set_width(tile.x);
        let (r, _) = ui.allocate_exact_size(tile, egui::Sense::hover());
        ui.painter().rect_filled(r, 3.0, egui::Color32::from_gray(30));
        match thumb {
            Thumb::Ready(t) => {
                let size = t.size_vec2();
                let fit = (tile.x / size.x).min(tile.y / size.y);
                let rect = egui::Rect::from_center_size(r.center(), size * fit);
                egui::Image::new(t).corner_radius(3.0).paint_at(ui, rect);
            }
            Thumb::Loading | Thumb::Unreadable => {
                let text = if matches!(thumb, Thumb::Unreadable) { "unreadable" } else { "…" };
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(10.0), egui::Color32::from_gray(110));
            }
        }
        ui.add(egui::Label::new(egui::RichText::new(name).small()).truncate());
    })
    .response
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

/// Shrinks an image to fit `max`, keeping its aspect ratio. HDR images (float pixels: HDR, EXR)
/// are tone-mapped as the viewport shows a material, at exposure 0 (shaders/tonemap.wgsl).
fn thumbnail(img: &image::DynamicImage, max: [u32; 2]) -> egui::ColorImage {
    if !matches!(img, image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_)) {
        let small = img.thumbnail(max[0], max[1]).to_rgba8();
        return egui::ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw());
    }
    // image's thumbnail() offsets float pixels by 0.5 and resize() clamps them to 1, so float
    // images are box-filtered here, in linear light.
    let small = match img {
        image::DynamicImage::ImageRgb32F(i) => shrink(i.dimensions(), max, |x, y| {
            let p = i.get_pixel(x, y);
            [p[0], p[1], p[2], 1.0]
        }),
        image::DynamicImage::ImageRgba32F(i) => shrink(i.dimensions(), max, |x, y| i.get_pixel(x, y).0),
        _ => unreachable!(),
    };
    let size = [small.width() as usize, small.height() as usize];
    let px: Vec<u8> = small
        .pixels()
        .flat_map(|p| {
            let c = |v: f32| (srgb_encode(aces(v.max(0.0) * 0.8)) * 255.0 + 0.5) as u8;
            [c(p[0]), c(p[1]), c(p[2]), (p[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]
        })
        .collect();
    egui::ColorImage::from_rgba_unmultiplied(size, &px)
}

/// Box-filters a `w`×`h` image, read through `pixel` (so large images aren't copied), down to
/// fit `max`, keeping its aspect ratio (never enlarges).
fn shrink((w, h): (u32, u32), max: [u32; 2], pixel: impl Fn(u32, u32) -> [f32; 4]) -> image::Rgba32FImage {
    let scale = (max[0] as f64 / w as f64).min(max[1] as f64 / h as f64).min(1.0);
    let (tw, th) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    image::Rgba32FImage::from_fn(tw, th, |x, y| {
        let (x0, x1) = (x * w / tw, ((x + 1) * w / tw).max(x * w / tw + 1));
        let (y0, y1) = (y * h / th, ((y + 1) * h / th).max(y * h / th + 1));
        let mut sum = [0.0f32; 4];
        for sy in y0..y1 {
            for sx in x0..x1 {
                let p = pixel(sx, sy);
                for c in 0..4 {
                    sum[c] += p[c];
                }
            }
        }
        let n = ((x1 - x0) * (y1 - y0)) as f32;
        image::Rgba(sum.map(|s| s / n))
    })
}

/// Narkowicz 2015 ACES fit, as in shaders/tonemap.wgsl.
fn aces(x: f32) -> f32 {
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

fn srgb_encode(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_keep_the_aspect_ratio() {
        let img = image::DynamicImage::new_rgb8(400, 100);
        assert_eq!(thumbnail(&img, [144, 144]).size, [144, 36]);
        let pano = image::DynamicImage::new_rgb32f(2048, 1024);
        assert_eq!(thumbnail(&pano, [288, 144]).size, [288, 144]);
    }

    #[test]
    fn shrink_averages_without_clamping() {
        let img = image::Rgba32FImage::from_fn(4, 2, |x, _| image::Rgba([[0.0, 2.0, 10.0, 30.0][x as usize], 0.0, 0.0, 1.0]));
        let s = shrink(img.dimensions(), [2, 2], |x, y| img.get_pixel(x, y).0);
        assert_eq!(s.dimensions(), (2, 1));
        assert_eq!((s.get_pixel(0, 0)[0], s.get_pixel(1, 0)[0]), (1.0, 20.0));
        // Never enlarges.
        assert_eq!(shrink(img.dimensions(), [100, 100], |x, y| img.get_pixel(x, y).0).dimensions(), (4, 2));
    }

    #[test]
    fn hdr_thumbnails_are_tone_mapped() {
        // Uniform images, so resampling doesn't mix the values.
        let r: Vec<u8> = [0.0, 0.18, 1.0, 50.0]
            .iter()
            .map(|&v| {
                let img = image::Rgb32FImage::from_pixel(8, 4, image::Rgb([v; 3]));
                thumbnail(&image::DynamicImage::ImageRgb32F(img), [4, 2]).pixels[0].r()
            })
            .collect();
        // Black stays black, very bright saturates, mid-gray is brighter than linear 0.18 * 255.
        assert_eq!(r[0], 0);
        assert_eq!(r[3], 255);
        assert!(r[1] > 46 && r[1] < r[2] && r[2] < 255, "{r:?}");
        // Same curve as the viewport: aces(0.8) encoded.
        assert_eq!(r[2], (srgb_encode(aces(0.8)) * 255.0 + 0.5) as u8);
    }
}
