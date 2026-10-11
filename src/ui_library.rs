//! Library panel: project resources (baked mesh maps, images) and the user library, one
//! category per library folder. See docs/reference/windows-and-panels.md "Library".

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

use egui::{Align2, Color32, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::gpu::Gpu;
use crate::icons::{self, Icon};
use crate::library::{self, Kind};
use crate::meshmaps::{MapStatus, MeshMap};
use crate::project::Project;
use crate::ui_theme as t;
use crate::ui_widgets as w;

const THUMB: usize = 256;
/// Thumbnails of image files are made to fit these (points), at THUMB_SCALE pixels per point:
/// square for images, 2:1 for environments (equirectangular panoramas).
const THUMB_TILE: Vec2 = vec2(TILE_MAX, TILE_MAX);
const THUMB_WIDE: Vec2 = vec2(TILE_MAX * 2.0, TILE_MAX);
/// Thumbnails are made at about twice the tile size, for high-DPI screens.
const THUMB_SCALE: f32 = 2.0;
/// Tiles: the gap between them, and the range and default of the preview size (a square
/// tile's width, in points), set with the size slider and remembered between sessions.
const GAP: f32 = 8.0;
const TILE_MIN: f32 = 40.0;
const TILE_MAX: f32 = 160.0;
const TILE_DEFAULT: f32 = 58.0;
const SLIDER_W: f32 = 64.0;
/// The name under a tile.
const NAME_GAP: f32 = 4.0;
const NAME_H: f32 = 13.0;
const NAME_SIZE: f32 = 10.5;
/// Category tabs: height, and the narrowest a tab gets before the tabs wrap to another row.
const TAB_H: f32 = 34.0;
const MIN_TAB: f32 = 26.0;
const SEARCH_H: f32 = 28.0;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Category {
    MeshMaps,
    Images,
    /// A folder of the user library.
    Library(String),
}

impl Category {
    fn title(&self) -> String {
        match self {
            Category::MeshMaps => "Mesh Maps".into(),
            Category::Images => "Images".into(),
            Category::Library(f) => library::title(f),
        }
    }

    fn icon(&self) -> Icon {
        let Category::Library(f) = self else {
            return if *self == Category::MeshMaps { Icon::MeshData } else { Icon::Image };
        };
        match library::kind(f) {
            Kind::SmartMaterials => Icon::NodeMaterial,
            Kind::Textures => Icon::Generator,
            Kind::Alphas => Icon::Alpha,
            Kind::Stencils => Icon::Stencil,
            Kind::Environments => Icon::World,
            Kind::Images => Icon::Folder,
            Kind::Reserved => match library::title(f).as_str() {
                "Materials" => Icon::Material,
                "Smart Masks" => Icon::Mask,
                _ => Icon::BrushData,
            },
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
    UseAlpha(PathBuf),
    LoadStencil(PathBuf),
    LoadEnvironment(PathBuf),
    /// Load a texture into the project's images.
    ImportTexture(PathBuf),
}

/// What a texture tile carries while dragged. Dropped onto the Node Editor, a file adds an Image
/// node and a preset its Procedural node. Not a bare PathBuf, which the Layers panel takes for a
/// smart material.
pub enum TextureDrop {
    File(PathBuf),
    /// An index into procedural::PRESETS.
    Preset(usize),
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
    fn new(ctx: &egui::Context, items: Vec<library::Item>, tile: Option<Vec2>) -> Self {
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
    /// Filters the shown category by name; cleared when another category is chosen.
    search: String,
    /// Set by the search shortcut: focus the search field next frame.
    focus_search: bool,
    /// The preview size, in points.
    tile: f32,
    smart: Option<Vec<(crate::smartmat::LibraryItem, Option<egui::TextureHandle>)>>,
    /// Folders the user added, scanned when first shown and on Refresh.
    extra: Option<Vec<String>>,
    folders: HashMap<String, FolderView>,
    /// Project images (name, source) and their thumbnails; rebuilt when the list changes.
    images: Option<(Vec<(String, PathBuf)>, FolderView)>,
    /// Thumbnails of the procedural texture presets, rendered when Textures is first shown.
    procedural: Option<Vec<egui::TextureHandle>>,
    /// Mesh map thumbnails with the maps version they were made from.
    map_thumbs: [Option<(u64, egui::TextureHandle)>; 8],
}

impl Default for Library {
    fn default() -> Self {
        Self {
            category: Category::MeshMaps,
            search: String::new(),
            focus_search: false,
            tile: TILE_DEFAULT,
            smart: None,
            extra: None,
            folders: HashMap::new(),
            images: None,
            procedural: None,
            map_thumbs: Default::default(),
        }
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

    /// Focuses the search field (the Search library shortcut).
    pub fn focus_search(&mut self) {
        self.focus_search = true;
    }

    /// `search_key`: the Search library shortcut, shown in the search field.
    pub fn ui(&mut self, ui: &mut Ui, gpu: &Gpu, p: &Project, search_key: &egui::KeyboardShortcut) -> Option<LibraryAction> {
        let extra = self.extra.get_or_insert_with(library::extra_folders).clone();
        if let Category::Library(f) = &self.category {
            if library::kind(f) == Kind::Images && !extra.contains(f) {
                // The folder was removed.
                self.category = Category::Library(library::BUILT_IN[0].0.into());
            }
        }
        let folders = library::BUILT_IN.iter().map(|b| b.0.to_string()).chain(extra);
        let cats: Vec<Category> = [Category::MeshMaps, Category::Images].into_iter().chain(folders.map(Category::Library)).collect();
        let before = self.category.clone();
        category_tabs(ui, &mut self.category, &cats);
        if self.category != before {
            self.search.clear();
        }
        ui.add_space(10.0);
        let hint = format!("Search {}", self.category.title().to_lowercase());
        let size_id = egui::Id::new("library preview size");
        self.tile = ui.data_mut(|d| *d.get_persisted_mut_or(size_id, TILE_DEFAULT)).clamp(TILE_MIN, TILE_MAX);
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), SEARCH_H), Sense::hover());
        let (field, slider) = row.split_left_right_at_x(row.max.x - SLIDER_W - 8.0);
        search_field(ui, field, &mut self.search, &mut self.focus_search, &hint, search_key);
        size_slider(ui, slider.with_min_x(slider.max.x - SLIDER_W), &mut self.tile);
        ui.add_space(6.0);
        let query = self.search.trim().to_lowercase();
        let matches = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
        let action = match self.category.clone() {
            Category::MeshMaps => self.mesh_maps(ui, gpu, p, &matches),
            Category::Images => {
                self.images(ui, p, &matches);
                None
            }
            Category::Library(folder) => {
                let (action, refresh) =
                    if library::kind(&folder) == Kind::SmartMaterials { self.smart_materials(ui, &matches) } else { self.folder(ui, gpu, &folder, &matches) };
                if refresh {
                    self.refresh();
                }
                action
            }
        };
        // Ctrl + wheel (or a pinch) over the Library changes the preview size too.
        if ui.rect_contains_pointer(ui.min_rect()) {
            let zoom = ui.input(|i| if i.modifiers.command { i.zoom_delta() } else { 1.0 });
            if zoom != 1.0 {
                self.tile = (self.tile * zoom).clamp(TILE_MIN, TILE_MAX);
                ui.ctx().request_repaint();
            }
        }
        ui.data_mut(|d| d.insert_persisted(size_id, self.tile));
        action
    }

    fn mesh_maps(&mut self, ui: &mut Ui, gpu: &Gpu, p: &Project, matches: &dyn Fn(&str) -> bool) -> Option<LibraryAction> {
        let shown: Vec<MeshMap> = MeshMap::ALL.into_iter().filter(|m| matches(m.name())).collect();
        info_row(ui, &Category::MeshMaps, shown.len(), MeshMap::ALL.len());
        if shown.is_empty() {
            no_matches(ui);
            return None;
        }
        let mut action = None;
        let mut read_one = false;
        let has_layer = p.selection.is_some();
        tile_grid(ui, self.tile, 1, shown.len(), |ui, i, size| {
            let m = shown[i];
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
            let name_color = if status == MapStatus::OutOfDate { t::ATTENTION } else { t::TEXT_2 };
            let thumb = &self.map_thumbs[m.index()];
            let resp = ui
                .dnd_drag_source(egui::Id::new(("meshmap", m.index())), m, |ui| {
                    tile(ui, size, m.name(), name_color, |ui, r| match (thumb, status) {
                        (Some((_, tex)), s) if s != MapStatus::Missing => {
                            egui::Image::new(tex).corner_radius(t::R_CONTROL).paint_at(ui, r);
                        }
                        _ => placeholder(ui, r, "not baked"),
                    })
                })
                .response;
            let hint = match status {
                MapStatus::Missing => "Not baked yet: bake it in Window > Mesh Maps.\n",
                MapStatus::OutOfDate => "Out of date: rebake to match the current settings.\n",
                MapStatus::UpToDate => "",
            };
            let resp = resp.on_hover_text(format!(
                "{}\n{hint}Double-click: add as a mask effect of the selected layer\nDrag onto the Node Editor: add a Mesh Map node",
                m.name()
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
        });
        action
    }

    /// Returns the action and whether Refresh was clicked.
    fn smart_materials(&mut self, ui: &mut Ui, matches: &dyn Fn(&str) -> bool) -> (Option<LibraryAction>, bool) {
        let items = self.smart.get_or_insert_with(|| {
            crate::smartmat::list()
                .into_iter()
                .map(|it| {
                    let tex = it.thumbnail.clone().map(|img| ui.ctx().load_texture(format!("smat {}", it.name), img, Default::default()));
                    (it, tex)
                })
                .collect()
        });
        let shown: Vec<usize> = (0..items.len()).filter(|&i| matches(&items[i].0.name)).collect();
        let folder = library::BUILT_IN[0].0.to_string();
        let refresh = info_row(ui, &Category::Library(folder), shown.len(), items.len());
        if items.is_empty() {
            empty(ui, "None yet. Right-click a group > Save as smart material.");
            return (None, refresh);
        }
        if shown.is_empty() {
            no_matches(ui);
            return (None, refresh);
        }
        let mut chosen = None;
        tile_grid(ui, self.tile, 1, shown.len(), |ui, i, size| {
            let (it, tex) = &items[shown[i]];
            let resp = ui
                .dnd_drag_source(egui::Id::new(("smat", &it.path)), it.path.clone(), |ui| {
                    tile(ui, size, &it.name, t::TEXT_2, |ui, r| match tex {
                        Some(tex) => {
                            egui::Image::new(tex).corner_radius(t::R_CONTROL).paint_at(ui, r);
                        }
                        None => placeholder(ui, r, "no preview"),
                    })
                })
                .response
                .on_hover_text(format!("{}\nDrag onto the Layers panel, or double-click to add", it.name));
            if resp.double_clicked() {
                chosen = Some(it.path.clone());
            }
            resp.context_menu(|ui| {
                if ui.button("Add to layers").clicked() {
                    chosen = Some(it.path.clone());
                    ui.close();
                }
            });
        });
        (chosen.map(LibraryAction::InsertSmartMaterial), refresh)
    }

    /// Any library folder other than smart materials. Returns the action and whether Refresh
    /// was clicked.
    fn folder(&mut self, ui: &mut Ui, gpu: &Gpu, folder: &str, matches: &dyn Fn(&str) -> bool) -> (Option<LibraryAction>, bool) {
        let kind = library::kind(folder);
        let tile_kind = match kind {
            Kind::Textures | Kind::Alphas | Kind::Stencils | Kind::Images => Some(THUMB_TILE),
            Kind::Environments => Some(THUMB_WIDE),
            Kind::SmartMaterials | Kind::Reserved => None,
        };
        // Textures lists the procedural presets before the folder's files.
        let preset_thumbs = (kind == Kind::Textures).then(|| {
            let px = (THUMB_TILE.x * THUMB_SCALE) as u32;
            let ctx = ui.ctx().clone();
            let thumbs = self.procedural.get_or_insert_with(|| {
                let images = crate::procedural::thumbnails(gpu, px);
                images.into_iter().enumerate().map(|(i, img)| ctx.load_texture(format!("procedural {i}"), img, egui::TextureOptions::LINEAR)).collect()
            });
            thumbs.clone()
        });
        let presets: Vec<usize> = match &preset_thumbs {
            Some(_) => (0..crate::procedural::PRESETS.len()).filter(|&i| matches(crate::procedural::PRESETS[i].name)).collect(),
            None => Vec::new(),
        };
        let preset_count = preset_thumbs.as_ref().map_or(0, Vec::len);
        let view = self.folders.entry(folder.to_string()).or_insert_with(|| FolderView::new(ui.ctx(), library::scan(folder), tile_kind));
        view.poll(ui.ctx());
        let shown: Vec<usize> = (0..view.items.len()).filter(|&i| matches(&view.items[i].0.name)).collect();
        let refresh = info_row(ui, &Category::Library(folder.to_string()), presets.len() + shown.len(), preset_count + view.items.len());
        if view.items.is_empty() && preset_count == 0 {
            empty(ui, "Empty. Add files to the folder (Show in file browser), then Refresh.");
            return (None, refresh);
        }
        if shown.is_empty() && presets.is_empty() {
            no_matches(ui);
            return (None, refresh);
        }
        // What double-click and the context menu do; None lists the files without actions.
        let use_item: Option<(&str, fn(PathBuf) -> LibraryAction)> = match kind {
            Kind::Textures => Some(("Import into project", LibraryAction::ImportTexture)),
            Kind::Alphas => Some(("Use as brush alpha", LibraryAction::UseAlpha)),
            Kind::Stencils | Kind::Images => Some(("Load as stencil", LibraryAction::LoadStencil)),
            Kind::Environments => Some(("Load environment", LibraryAction::LoadEnvironment)),
            Kind::SmartMaterials | Kind::Reserved => None,
        };
        let mut action = None;
        let mut item_ui = |resp: Response, it: &library::Item| {
            let file = it.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let Some((label, make)) = use_item else {
                resp.on_hover_text(file);
                return;
            };
            let drag = if kind == Kind::Textures { "\nDrag onto the Node Editor: add an Image node" } else { "" };
            let resp = resp.on_hover_text(format!("{file}\nDouble-click: {}{drag}", label.to_lowercase()));
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
        if tile_kind.is_some() {
            let span = if kind == Kind::Environments { 2 } else { 1 };
            tile_grid(ui, self.tile, span, presets.len() + shown.len(), |ui, i, size| {
                if let (Some(&k), Some(thumbs)) = (presets.get(i), &preset_thumbs) {
                    let preset = &crate::procedural::PRESETS[k];
                    let title = crate::graph::def(preset.ty).map_or(preset.ty, |d| d.title);
                    ui.dnd_drag_source(egui::Id::new(("procedural", k)), TextureDrop::Preset(k), |ui| {
                        tile(ui, size, preset.name, t::TEXT_2, |ui, r| {
                            egui::Image::new(&thumbs[k]).corner_radius(t::R_CONTROL).paint_at(ui, r);
                        })
                    })
                    .response
                    .on_hover_text(format!("{}\nProcedural: a {title} node\nDrag onto the Node Editor: add it", preset.name));
                    return;
                }
                let (it, thumb) = &view.items[shown[i - presets.len()]];
                let resp = if kind == Kind::Textures {
                    let id = egui::Id::new(("texture", &it.path));
                    ui.dnd_drag_source(id, TextureDrop::File(it.path.clone()), |ui| tile(ui, size, &it.name, t::TEXT_2, |ui, r| paint_thumb(ui, r, thumb)))
                        .response
                } else {
                    tile(ui, size, &it.name, t::TEXT_2, |ui, r| paint_thumb(ui, r, thumb))
                };
                item_ui(resp, it);
            });
        } else {
            for &i in &shown {
                let it = &view.items[i].0;
                item_ui(list_row(ui, &it.name), it);
            }
        }
        (action, refresh)
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
    fn images(&mut self, ui: &mut Ui, p: &Project, matches: &dyn Fn(&str) -> bool) {
        // Sorted by name (a BTreeMap); an image without a source shows as unreadable.
        let list: Vec<(String, PathBuf)> =
            p.image_sizes.keys().map(|n| (n.clone(), p.image_sources.get(n).cloned().unwrap_or_default())).collect();
        if self.images.as_ref().is_none_or(|(l, _)| *l != list) {
            let items = list.iter().map(|(name, path)| library::Item { name: name.clone(), path: path.clone() }).collect();
            self.images = Some((list, FolderView::new(ui.ctx(), items, Some(THUMB_TILE))));
        }
        let Some((_, view)) = &mut self.images else { return };
        view.poll(ui.ctx());
        let shown: Vec<usize> = (0..view.items.len()).filter(|&i| matches(&view.items[i].0.name)).collect();
        info_row(ui, &Category::Images, shown.len(), view.items.len());
        if view.items.is_empty() {
            empty(ui, "No images imported.");
            return;
        }
        if shown.is_empty() {
            no_matches(ui);
            return;
        }
        tile_grid(ui, self.tile, 1, shown.len(), |ui, i, size| {
            let (it, thumb) = &view.items[shown[i]];
            let (w, h) = p.image_sizes.get(&it.name).copied().unwrap_or_default();
            let source = if it.path.as_os_str().is_empty() { "no source file".to_string() } else { it.path.display().to_string() };
            tile(ui, size, &it.name, t::TEXT_2, |ui, r| paint_thumb(ui, r, thumb)).on_hover_text(format!("{}\n{w}×{h}\n{source}", it.name));
        });
    }
}

/// One icon tab per category, filling the width (wrapping to more rows when they don't fit),
/// with a line between the project's categories and the library's.
fn category_tabs(ui: &mut Ui, current: &mut Category, cats: &[Category]) {
    let width = ui.available_width();
    let per_row = ((width / MIN_TAB).floor() as usize).clamp(1, cats.len().max(1));
    let rows = cats.len().div_ceil(per_row);
    let (rect, _) = ui.allocate_exact_size(vec2(width, TAB_H * rows as f32), Sense::hover());
    let p = ui.painter().clone();
    p.hline(rect.x_range(), rect.min.y + 0.5, Stroke::new(1.0, t::DIVIDER));
    p.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, t::BORDER));
    let cell_w = width / per_row as f32;
    for (i, c) in cats.iter().enumerate() {
        let (row, col) = (i / per_row, i % per_row);
        let r = Rect::from_min_size(rect.min + vec2(col as f32 * cell_w, row as f32 * TAB_H), vec2(cell_w, TAB_H));
        let resp = ui.interact(r, ui.id().with(("library tab", i)), Sense::click()).on_hover_text(c.title());
        if resp.clicked() {
            *current = c.clone();
        }
        let active = current == c;
        let color = if active {
            t::TEXT
        } else if resp.hovered() {
            t::TEXT_2
        } else {
            t::TEXT_3
        };
        icons::paint(&p, c.icon(), Rect::from_center_size(r.center(), Vec2::splat(16.0)), color);
        if active {
            p.rect_filled(Rect::from_min_max(pos2(r.min.x + 4.0, r.max.y - 2.0), pos2(r.max.x - 4.0, r.max.y)), 1.0, t::ACCENT);
        }
        if i == 2 && col != 0 {
            p.vline(r.min.x + 0.5, r.y_range(), Stroke::new(1.0, t::BORDER));
        }
    }
}

/// The search field: a magnifier, the query, and the shortcut while it's empty and unfocused
/// (a clear button once something is typed). `Esc` clears it.
fn search_field(ui: &mut Ui, rect: Rect, search: &mut String, focus: &mut bool, hint: &str, key: &egui::KeyboardShortcut) {
    let p = ui.painter().clone();
    let edit_id = ui.id().with("library search");
    let focused = ui.memory(|m| m.has_focus(edit_id));
    let border = if focused { t::accent(0.6) } else { t::BORDER };
    p.rect(rect, t::R_CONTROL, t::BG_APP, Stroke::new(1.0, border), StrokeKind::Inside);
    icons::paint(&p, Icon::Search, Rect::from_center_size(pos2(rect.min.x + 16.0, rect.center().y), Vec2::splat(14.0)), t::TEXT_3);
    let mut right = rect.max.x - 6.0;
    if !search.is_empty() {
        let r = Rect::from_center_size(pos2(right - 9.0, rect.center().y), Vec2::splat(18.0));
        let resp = ui.interact(r, edit_id.with("clear"), Sense::click()).on_hover_text("Clear search");
        icons::paint(&p, Icon::Close, r.shrink(3.0), if resp.hovered() { t::TEXT } else { t::TEXT_3 });
        if resp.clicked() {
            search.clear();
        }
        right = r.min.x - 2.0;
    } else if !focused {
        let text = crate::shortcuts::format(key).replace('+', " ");
        let galley = p.layout_no_wrap(text, FontId::monospace(t::FS_VALUE - 1.0), t::TEXT_2);
        let size = galley.size() + vec2(10.0, 2.0);
        let r = Rect::from_min_size(pos2(right - size.x, rect.center().y - size.y / 2.0), size);
        p.rect_filled(r, t::R_CHECK, t::BG_INPUT);
        p.galley(r.min + vec2(5.0, 1.0), galley, t::TEXT_2);
        right = r.min.x - 4.0;
    }
    let edit_rect = Rect::from_min_max(pos2(rect.min.x + 30.0, rect.min.y), pos2(right.max(rect.min.x + 40.0), rect.max.y));
    let edit = egui::TextEdit::singleline(search)
        .id(edit_id)
        .hint_text(RichText::new(hint).color(t::TEXT_3))
        .vertical_align(egui::Align::Center)
        .frame(egui::Frame::NONE)
        .desired_width(f32::INFINITY);
    // A detached child, so the field doesn't move this Ui's cursor.
    let resp = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect).layout(egui::Layout::left_to_right(egui::Align::Center))).add(edit);
    if std::mem::take(focus) {
        resp.request_focus();
        resp.scroll_to_me(None);
    }
    // The field gives up focus on Esc; Esc also clears it.
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        search.clear();
    }
}

/// The preview size: a small image icon and a thin track with a knob. Click or drag to set it;
/// double-click resets it.
fn size_slider(ui: &mut Ui, rect: Rect, value: &mut f32) {
    let resp = ui.interact(rect, ui.id().with("library preview size"), Sense::click_and_drag());
    let track = egui::Rangef::new(rect.min.x + 20.0, rect.max.x - 5.0);
    if resp.double_clicked() {
        *value = TILE_DEFAULT;
    } else if resp.is_pointer_button_down_on() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let f = ((pos.x - track.min) / track.span()).clamp(0.0, 1.0);
            *value = (TILE_MIN + f * (TILE_MAX - TILE_MIN)).round();
        }
    }
    let active = resp.hovered() || resp.dragged();
    let p = ui.painter();
    let y = rect.center().y;
    icons::paint(p, Icon::Image, Rect::from_center_size(pos2(rect.min.x + 7.0, y), Vec2::splat(12.0)), if active { t::TEXT_2 } else { t::TEXT_3 });
    let x = track.min + (*value - TILE_MIN) / (TILE_MAX - TILE_MIN) * track.span();
    p.hline(track, y, Stroke::new(2.0, t::BORDER_STRONG));
    p.hline(egui::Rangef::new(track.min, x), y, Stroke::new(2.0, t::TEXT_3));
    p.circle_filled(pos2(x, y), 5.0, if active { t::TEXT } else { t::TEXT_2 });
    resp.on_hover_text(format!("Preview size: {value:.0} pt
Double-click: reset"));
}

/// The category's name and where it lives, Refresh and Show in file browser for library
/// folders, and the number of items shown (of all, while searching). Returns whether Refresh
/// was clicked.
fn info_row(ui: &mut Ui, cat: &Category, shown: usize, total: usize) -> bool {
    let mut refresh = false;
    let size = vec2(ui.available_width(), t::H_CONTROL);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let scope = if matches!(cat, Category::Library(_)) { "Library" } else { "Project" };
        ui.label(RichText::new(cat.title()).font(t::caption()).color(t::TEXT_2));
        ui.label(RichText::new(format!(" · {scope}")).font(t::caption()).color(t::TEXT_3));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let count = if shown == total { total.to_string() } else { format!("{shown} / {total}") };
            ui.label(RichText::new(count).font(t::value()).color(t::TEXT_3));
            if let Category::Library(folder) = cat {
                ui.add_space(6.0);
                if ui.add(w::ghost_icon(Icon::FileBrowser)).on_hover_text("Show in file browser").clicked() {
                    let dir = library::dir(folder);
                    let _ = std::fs::create_dir_all(&dir);
                    crate::prefs::open_folder(&dir);
                }
                if ui.add(w::ghost_icon(Icon::Refresh)).on_hover_text("Refresh: rescan the library folders").clicked() {
                    refresh = true;
                }
            }
        });
    });
    ui.add_space(4.0);
    refresh
}

fn empty(ui: &mut Ui, text: &str) {
    crate::ui_paint::empty_state(ui, |ui| {
        w::hint(ui, text);
    });
}

fn no_matches(ui: &mut Ui) {
    empty(ui, "No matches.");
}

/// Lays out `n` tiles `tile` points wide (narrower only when the width can't hold one) in as
/// many columns as fit. A tile spans `span` columns; it's square for one, twice as wide as high
/// for two. `add` gets the tile's thumbnail size.
/// horizontal_wrapped can't be used: it decides where to wrap before a tile's size is known, so
/// its rows never wrap and widen the panel instead.
fn tile_grid(ui: &mut Ui, tile: f32, span: usize, n: usize, mut add: impl FnMut(&mut Ui, usize, Vec2)) {
    let width = ui.available_width();
    let cols = (((width + GAP) / (tile + GAP)).floor() as usize).max(span);
    let col_w = tile.min((width - GAP * (span - 1) as f32) / span as f32).floor().max(1.0);
    let tile_w = col_w * span as f32 + GAP * (span - 1) as f32;
    let size = vec2(tile_w, if span == 1 { tile_w } else { (tile_w / 2.0).round() });
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        for row in (0..n).collect::<Vec<_>>().chunks(cols / span) {
            ui.horizontal(|ui| {
                for &i in row {
                    add(ui, i, size);
                }
            });
        }
    });
}

/// A tile: a thumbnail area of `size` (rounded, filled by `paint`) and the name below,
/// truncated to the tile's width. Hovering outlines the thumbnail.
fn tile(ui: &mut Ui, size: Vec2, name: &str, name_color: Color32, paint: impl FnOnce(&Ui, Rect)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size.x, size.y + NAME_GAP + NAME_H), Sense::click());
    let thumb = Rect::from_min_size(rect.min, size);
    ui.painter().rect_filled(thumb, t::R_CONTROL, t::BG_INPUT);
    paint(ui, thumb);
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_stroke(thumb, t::R_CONTROL, Stroke::new(1.0, t::TEXT_3), StrokeKind::Inside);
    }
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), FontId::proportional(NAME_SIZE), name_color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(size.x);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.min.x, thumb.max.y + NAME_GAP), galley, name_color);
    resp
}

/// A thumbnail fitted into `r`, keeping its aspect ratio, or why there is none.
fn paint_thumb(ui: &Ui, r: Rect, thumb: &Thumb) {
    match thumb {
        Thumb::Ready(tex) => {
            let size = tex.size_vec2();
            let fit = (r.width() / size.x).min(r.height() / size.y);
            egui::Image::new(tex).corner_radius(t::R_CONTROL).paint_at(ui, Rect::from_center_size(r.center(), size * fit));
        }
        Thumb::Loading => placeholder(ui, r, "…"),
        Thumb::Unreadable => placeholder(ui, r, "unreadable"),
    }
}

fn placeholder(ui: &Ui, r: Rect, text: &str) {
    ui.painter().with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(10.0), t::TEXT_3);
}

/// A file listed by name (reserved folders): a row that highlights on hover.
fn list_row(ui: &mut Ui, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t::H_CONTROL), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, t::R_CONTROL, t::BG_INPUT);
    }
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), t::body(), t::TEXT_2);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 16.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos2(rect.min.x + 8.0, rect.center().y - galley.size().y / 2.0), galley, t::TEXT_2);
    resp
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
