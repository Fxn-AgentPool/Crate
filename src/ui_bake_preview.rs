//! The bake preview pane of the Mesh Maps window: part toggles and opacities, and its own view
//! of the low poly, high poly and cage (bakeview.rs). See docs/reference/mesh-maps.md#bake-preview.

use egui::Vec2;

use crate::bakepreview::Part;
use crate::bakeview::BakeView;
use crate::camera::Camera;
use crate::gpu::Gpu;
use crate::project::Project;

/// Room left under the view for the loading or error line, in points.
const STATUS_HEIGHT: f32 = 22.0;

pub struct BakePane {
    view: Option<BakeView>,
    pub camera: Camera,
    texture: Option<(egui::TextureId, u32, u32)>,
}

impl BakePane {
    pub fn new() -> Self {
        Self { view: None, camera: Camera::default(), texture: None }
    }

    /// Frames the mesh, a little tighter than its bounding sphere: the preview is small.
    pub fn frame(&mut self, mesh: &crate::mesh::Mesh) {
        self.camera.frame(mesh.center(), mesh.radius());
        self.camera.distance *= 0.7;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        gpu: &Gpu,
        renderer: &std::sync::Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
        doc: &mut Project,
    ) {
        let has_high = doc.maps.high_poly.is_some();
        let parts = &mut doc.maps.preview.parts;
        egui::Grid::new("bake preview parts").num_columns(2).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, part: &mut Part, name: &str, color: egui::Color32, enabled: bool| {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, color);
                        ui.checkbox(&mut part.show, name);
                    });
                })
                .response
                .on_disabled_hover_text("Needs a high-poly mesh.");
                ui.add_enabled(enabled && part.show, egui::Slider::new(&mut part.opacity, 0.0..=1.0).text("Opacity"));
                ui.end_row();
            };
            row(ui, &mut parts.low, "Low poly", egui::Color32::from_rgb(80, 150, 255), true);
            row(ui, &mut parts.high, "High poly", egui::Color32::from_gray(235), has_high);
            row(ui, &mut parts.cage, "Cage", egui::Color32::from_rgb(255, 80, 60), has_high);
        });

        // The view fills the rest of the pane.
        let size = (ui.available_size() - Vec2::new(0.0, STATUS_HEIGHT)).max(Vec2::splat(64.0));
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let shift = ui.input(|i| i.modifiers.shift);
        let delta = response.drag_delta();
        let delta = glam::Vec2::new(delta.x, delta.y);
        if response.dragged_by(egui::PointerButton::Middle) || (response.dragged_by(egui::PointerButton::Primary) && shift) {
            self.camera.pan(delta, rect.height());
        } else if response.dragged_by(egui::PointerButton::Primary) {
            self.camera.orbit(delta);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.dolly(0.999f32.powf(scroll));
            }
        }
        if response.double_clicked() {
            self.frame(&doc.mesh);
        }

        let ppp = ui.ctx().pixels_per_point();
        let (w, h) = ((rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32);
        let view = self.view.get_or_insert_with(|| BakeView::new(gpu));
        view.render(gpu, &self.camera, doc.mesh.radius(), &doc.maps.preview, &doc.gpu_mesh, w, h);
        let output = view.output_view().unwrap();
        let id = match self.texture {
            Some((id, tw, th)) if tw == w && th == h => id,
            Some((id, _, _)) => {
                renderer.write().update_egui_texture_from_wgpu_texture(&gpu.device, output, wgpu::FilterMode::Linear, id);
                self.texture = Some((id, w, h));
                id
            }
            None => {
                let id = renderer.write().register_native_texture(&gpu.device, output, wgpu::FilterMode::Linear);
                self.texture = Some((id, w, h));
                id
            }
        };
        ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);

        let preview = &doc.maps.preview;
        if preview.loading() {
            ui.label("Loading preview…");
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else if let Some(e) = preview.error() {
            ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
        }
    }
}
