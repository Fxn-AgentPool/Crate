//! Smaller windows: Performance, Preferences and Project > Channels.

use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::prefs::Preferences;
use crate::project::Project;

#[derive(Default)]
pub struct Windows {
    pub performance: bool,
    pub preferences: bool,
    pub channels: bool,
    /// Smoothed frame time in seconds.
    pub frame_time: f32,
    /// Action waiting for a new key combination.
    capture: Option<crate::shortcuts::Action>,
}

fn gb(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// GPU memory held by paint pools, mask pool, caches and mesh maps.
pub fn gpu_memory(p: &Project) -> (u64, u64) {
    let pool = |pl: &crate::scene::Pool| pl.res as u64 * pl.res as u64 * pl.layers as u64 * 8;
    let paint: u64 = p.scene.paint.iter().map(pool).sum::<u64>() + pool(&p.scene.mask);
    let caches = pool(&p.scene.cache);
    (paint, caches)
}

impl Windows {
    pub fn capturing_shortcut(&self) -> bool {
        self.capture.is_some()
    }

    /// Returns true when the shortcut bindings changed.
    pub fn show(&mut self, ctx: &egui::Context, gpu: &Gpu, p: Option<&mut Project>, prefs: &mut Preferences, height_scale: &mut f32) -> bool {
        let mut bindings_changed = false;
        let dt = ctx.input(|i| i.unstable_dt);
        self.frame_time = self.frame_time * 0.9 + dt * 0.1;

        if self.preferences {
            let mut open = true;
            egui::Window::new("Preferences").open(&mut open).show(ctx, |ui| {
                ui.heading("Performance");
                let before = prefs.clone();
                egui::Grid::new("prefs").num_columns(2).show(ui, |ui| {
                    ui.label("GPU cache budget");
                    ui.add(egui::DragValue::new(&mut prefs.gpu_cache_budget_gb).range(0.5..=128.0).speed(0.1).suffix(" GB"));
                    ui.end_row();
                    ui.label("Undo budget");
                    ui.add(egui::DragValue::new(&mut prefs.undo_budget_gb).range(0.1..=64.0).speed(0.1).suffix(" GB"));
                    ui.end_row();
                    ui.label("Undo steps");
                    ui.add(egui::DragValue::new(&mut prefs.undo_steps).range(1..=1000));
                    ui.end_row();
                });
                ui.separator();
                ui.heading("Shortcuts");
                let bindings = crate::shortcuts::Bindings::new(&prefs.shortcuts);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("shortcuts").num_columns(2).striped(true).show(ui, |ui| {
                        for a in crate::shortcuts::Action::ALL {
                            ui.label(a.label());
                            let text = if self.capture == Some(a) {
                                "Press keys…".to_string()
                            } else {
                                crate::shortcuts::format(&bindings.get(a))
                            };
                            if ui.button(text).clicked() {
                                self.capture = Some(a);
                            }
                            ui.end_row();
                        }
                    });
                });
                if ui.button("Reset to defaults").clicked() {
                    prefs.shortcuts.clear();
                }
                if let Some(a) = self.capture {
                    let pressed = ui.input(|i| {
                        i.events.iter().find_map(|e| match e {
                            egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                            _ => None,
                        })
                    });
                    if let Some((key, mods)) = pressed {
                        if key == egui::Key::Escape {
                            self.capture = None;
                        } else {
                            let sc = egui::KeyboardShortcut::new(mods, key);
                            prefs.shortcuts.insert(a, crate::shortcuts::format(&sc));
                            self.capture = None;
                        }
                    }
                }
                if *prefs != before {
                    bindings_changed |= prefs.shortcuts != before.shortcuts;
                    prefs.save();
                }
            });
            self.preferences = open;
            if !open {
                self.capture = None;
            }
        }

        let Some(p) = p else { return bindings_changed };
        p.undo_limit = prefs.undo_steps;
        p.undo_bytes = (prefs.undo_budget_gb as f64 * (1u64 << 30) as f64) as usize;

        if self.performance {
            let mut open = true;
            egui::Window::new("Performance").open(&mut open).show(ctx, |ui| {
                egui::Grid::new("perf").num_columns(2).show(ui, |ui| {
                    ui.label("Frame time");
                    ui.label(format!("{:.1} ms ({:.0} fps)", self.frame_time * 1000.0, 1.0 / self.frame_time.max(1e-4)));
                    ui.end_row();
                    ui.label("Shader length");
                    ui.label(format!("{} lines", p.material_code.lines().count()));
                    ui.end_row();
                    let (paint, caches) = gpu_memory(p);
                    let budget = (prefs.gpu_cache_budget_gb as f64 * (1u64 << 30) as f64) as u64;
                    ui.label("GPU paint");
                    ui.label(gb(paint));
                    ui.end_row();
                    ui.label("GPU cache");
                    ui.label(format!("{} / {}", gb(caches + paint), gb(budget)));
                    ui.end_row();
                    if caches + paint > budget {
                        ui.label("");
                        ui.colored_label(egui::Color32::from_rgb(230, 180, 70), "Over budget");
                        ui.end_row();
                    }
                });
                if let Some(e) = &p.cache_error {
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
                }
                ui.separator();
                ui.strong("Cached nodes");
                if p.caches.entries.is_empty() {
                    ui.label(egui::RichText::new("None").weak());
                }
                egui::Grid::new("caches").num_columns(3).striped(true).show(ui, |ui| {
                    for e in p.caches.entries.values() {
                        let name = p.graph.get(e.node).map(|n| n.title().to_string()).unwrap_or_default();
                        ui.label(name);
                        ui.label(if e.signature.is_some() { "ready" } else { "pending" });
                        ui.label(format!("{:.1} ms", e.cost_ms));
                        ui.end_row();
                    }
                });
            });
            self.performance = open;
        }

        if self.channels {
            let mut open = true;
            egui::Window::new("Channels").open(&mut open).show(ctx, |ui| {
                let before = p.channels.clone();
                let ch = &mut p.channels;
                egui::Grid::new("channels").num_columns(2).show(ui, |ui| {
                    for c in Channel::ALL {
                        let i = c.index();
                        ui.checkbox(&mut ch.enabled[i], c.name());
                        ui.add_enabled_ui(ch.enabled[i], |ui| match c {
                            Channel::BaseColor | Channel::Emissive => {
                                ui.color_edit_button_rgb(&mut ch.defaults[i]);
                            }
                            Channel::Normal => {
                                ui.horizontal(|ui| {
                                    for k in 0..3 {
                                        ui.add(egui::DragValue::new(&mut ch.defaults[i][k]).range(0.0..=1.0).speed(0.01));
                                    }
                                });
                            }
                            Channel::Height => {
                                ui.add(egui::Slider::new(&mut ch.defaults[i][0], -1.0..=1.0));
                            }
                            _ => {
                                ui.add(egui::Slider::new(&mut ch.defaults[i][0], 0.0..=1.0));
                            }
                        });
                        ui.end_row();
                    }
                    ui.label("Height scale");
                    ui.add(egui::DragValue::new(&mut ch.height_scale).range(0.0001..=10.0).speed(0.0005).suffix(" units"));
                    ui.end_row();
                });
                if p.channels != before {
                    *height_scale = p.channels.height_scale;
                    p.graph_changed(gpu);
                }
            });
            self.channels = open;
        }
        bindings_changed
    }
}
