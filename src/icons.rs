//! Interface icons from Blender (src/icons, see NOTICE.md), rasterized at the size they're drawn.
//! Most are Blender's white SVG icons, tinted when painted. The tools are Blender's toolbar icons:
//! colored triangle geometry, painted in their own colors and only darkened by the tint.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, TextureHandle, pos2, vec2};
use resvg::{tiny_skia, usvg};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    Brush,
    Fill,
    Folder,
    Mask,
    Effect,
    Duplicate,
    Trash,
    Nodes,
    Visible,
    Hidden,
    Levels,
    Material,
    Generator,
    Erase,
    Stencil,
    Close,
    Remove,
    MeshData,
    Image,
    NodeMaterial,
    BrushData,
    Alpha,
    World,
    Search,
    Refresh,
    FileBrowser,
    Bake,
    GeoFill,
    Curve,
}

enum Source {
    Svg(&'static [u8]),
    /// Blender's toolbar icon geometry (release/datafiles/icons/*.dat).
    Geometry(&'static [u8]),
}

impl Icon {
    fn source(self) -> Source {
        use Source::{Geometry, Svg};
        match self {
            Icon::Brush => Geometry(include_bytes!("icons/brush.generic.dat")),
            Icon::Fill => Geometry(include_bytes!("icons/brush.paint_texture.fill.dat")),
            Icon::Folder => Svg(include_bytes!("icons/file_folder.svg")),
            Icon::Mask => Svg(include_bytes!("icons/mod_mask.svg")),
            Icon::Effect => Svg(include_bytes!("icons/shaderfx.svg")),
            Icon::Duplicate => Svg(include_bytes!("icons/duplicate.svg")),
            Icon::Trash => Svg(include_bytes!("icons/trash.svg")),
            Icon::Nodes => Svg(include_bytes!("icons/nodetree.svg")),
            Icon::Visible => Svg(include_bytes!("icons/hide_off.svg")),
            Icon::Hidden => Svg(include_bytes!("icons/hide_on.svg")),
            Icon::Levels => Svg(include_bytes!("icons/seq_histogram.svg")),
            Icon::Material => Svg(include_bytes!("icons/material.svg")),
            Icon::Generator => Svg(include_bytes!("icons/texture.svg")),
            Icon::Erase => Geometry(include_bytes!("icons/brush.gpencil_draw.erase.dat")),
            Icon::Stencil => Svg(include_bytes!("icons/image_reference.svg")),
            Icon::Close => Svg(include_bytes!("icons/x.svg")),
            Icon::Remove => Svg(include_bytes!("icons/remove.svg")),
            Icon::MeshData => Svg(include_bytes!("icons/mesh_data.svg")),
            Icon::Image => Svg(include_bytes!("icons/image_data.svg")),
            Icon::NodeMaterial => Svg(include_bytes!("icons/node_material.svg")),
            Icon::BrushData => Svg(include_bytes!("icons/brush_data.svg")),
            Icon::Alpha => Svg(include_bytes!("icons/image_alpha.svg")),
            Icon::World => Svg(include_bytes!("icons/world.svg")),
            Icon::Search => Svg(include_bytes!("icons/viewzoom.svg")),
            Icon::Refresh => Svg(include_bytes!("icons/file_refresh.svg")),
            Icon::FileBrowser => Svg(include_bytes!("icons/filebrowser.svg")),
            Icon::Bake => Svg(include_bytes!("icons/mod_uvproject.svg")),
            Icon::GeoFill => Svg(include_bytes!("icons/facesel.svg")),
            Icon::Curve => Svg(include_bytes!("icons/curve_bezcurve.svg")),
        }
    }
}

/// Blender draws its icons on a 16-pixel grid at 100 SVG units per pixel. Scaling every icon by
/// the same factor keeps line weights alike; the files are cropped, so each is centered.
const GRID: f32 = 1600.0;

/// Textures by icon and size in physical pixels.
#[derive(Clone, Default)]
struct Cache(Arc<HashMap<(Icon, u32), TextureHandle>>);

/// Paints `icon` centered in `r`, as large as fits in a square, in `color`.
pub fn paint(p: &egui::Painter, icon: Icon, r: Rect, color: Color32) {
    let ppp = p.pixels_per_point();
    let px = (r.width().min(r.height()) * ppp).round().max(1.0);
    // Snapped to whole pixels so the raster isn't resampled.
    let snap = |v: f32| (v * ppp - px / 2.0).round() / ppp;
    let min = pos2(snap(r.center().x), snap(r.center().y));
    let rect = Rect::from_min_size(min, vec2(px, px) / ppp);
    let tex = texture(p.ctx(), icon, px as u32);
    p.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), color);
}

fn texture(ctx: &egui::Context, icon: Icon, px: u32) -> TextureHandle {
    let id = egui::Id::new("icon textures");
    if let Some(t) = ctx.data(|d| d.get_temp::<Cache>(id)).and_then(|c| c.0.get(&(icon, px)).cloned()) {
        return t;
    }
    let tex = ctx.load_texture(format!("icon {icon:?} {px}"), rasterize(icon, px), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(id);
        Arc::make_mut(&mut cache.0).insert((icon, px), tex.clone());
    });
    tex
}

/// The icon on transparent, `px` pixels square: white for SVG icons, colored for geometry.
fn rasterize(icon: Icon, px: u32) -> egui::ColorImage {
    let mut pixmap = tiny_skia::Pixmap::new(px, px).expect("icon size is positive");
    match icon.source() {
        Source::Svg(data) => match usvg::Tree::from_data(data, &usvg::Options::default()) {
            Ok(tree) => {
                let size = tree.size();
                let k = px as f32 / GRID.max(size.width()).max(size.height());
                let dx = (px as f32 - size.width() * k) / 2.0;
                let dy = (px as f32 - size.height() * k) / 2.0;
                resvg::render(&tree, tiny_skia::Transform::from_scale(k, k).post_translate(dx, dy), &mut pixmap.as_mut());
            }
            // The icons are compiled in, so a bad file is a bug, not a user error.
            Err(e) => log::error!("icon {icon:?}: {e}"),
        },
        Source::Geometry(data) => match geometry(data) {
            Some(groups) => {
                // Toolbar icons leave a wide margin in their canvas; fitting the shape itself
                // makes them as large as the SVG icons beside them.
                let pts = || groups.iter().flat_map(|(_, tris)| tris.iter().flatten());
                let (x0, x1) = pts().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
                let (y0, y1) = pts().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
                let k = px as f32 / (x1 - x0).max(y1 - y0).max(1.0);
                let (dx, dy) = ((px as f32 - (x1 - x0) * k) / 2.0, (px as f32 - (y1 - y0) * k) / 2.0);
                // Blender's y axis points up.
                let at = |(x, y): (f32, f32)| ((x - x0) * k + dx, (y1 - y) * k + dy);
                for (color, tris) in groups {
                    // One path per color, so the edges between its triangles don't show as seams.
                    let mut pb = tiny_skia::PathBuilder::new();
                    for t in tris {
                        let [a, b, c] = t.map(at);
                        pb.move_to(a.0, a.1);
                        pb.line_to(b.0, b.1);
                        pb.line_to(c.0, c.1);
                        pb.close();
                    }
                    let Some(path) = pb.finish() else { continue };
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
                    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
                }
            }
            None => log::error!("icon {icon:?}: not Blender icon geometry"),
        },
    }
    egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data())
}

type Triangle = [(f32, f32); 3];

/// Parses Blender's icon geometry ("VCO": an 8-byte header, then 8-bit x, y per vertex and RGBA
/// per vertex, three vertices a triangle) into triangles grouped by color. Each triangle is one
/// color in Blender's icons; the first vertex's is used.
fn geometry(data: &[u8]) -> Option<Vec<([u8; 4], Vec<Triangle>)>> {
    let body = data.strip_prefix(b"VCO\0")?.get(4..)?;
    if body.is_empty() || body.len() % 18 != 0 {
        return None;
    }
    let n = body.len() / 18;
    let (coords, colors) = body.split_at(n * 6);
    let mut groups: Vec<([u8; 4], Vec<Triangle>)> = Vec::new();
    for i in 0..n {
        let v = |k: usize| (coords[i * 6 + k * 2] as f32, coords[i * 6 + k * 2 + 1] as f32);
        let color: [u8; 4] = colors[i * 12..i * 12 + 4].try_into().ok()?;
        if color[3] == 0 {
            continue;
        }
        let tri = [v(0), v(1), v(2)];
        match groups.iter_mut().find(|(c, _)| *c == color) {
            Some((_, tris)) => tris.push(tri),
            None => groups.push((color, vec![tri])),
        }
    }
    (!groups.is_empty()).then_some(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 27] = [
        Icon::Brush,
        Icon::Fill,
        Icon::Folder,
        Icon::Mask,
        Icon::Effect,
        Icon::Duplicate,
        Icon::Trash,
        Icon::Nodes,
        Icon::Visible,
        Icon::Hidden,
        Icon::Levels,
        Icon::Material,
        Icon::Generator,
        Icon::Erase,
        Icon::Stencil,
        Icon::Close,
        Icon::Remove,
        Icon::MeshData,
        Icon::Image,
        Icon::NodeMaterial,
        Icon::BrushData,
        Icon::Alpha,
        Icon::World,
        Icon::Search,
        Icon::Refresh,
        Icon::FileBrowser,
        Icon::Bake,
    ];

    #[test]
    fn every_icon_draws_something() {
        for icon in ALL {
            let img = rasterize(icon, 16);
            assert!(img.pixels.iter().any(|p| p.a() > 0), "{icon:?} is empty");
        }
    }
}
