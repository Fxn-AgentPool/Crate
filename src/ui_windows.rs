//! Smaller windows: Performance, Preferences and Project > Channels.

use crate::ui_widgets::{Check, Field, Section, Toggle};
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
            egui::Window::new("Preferences").open(&mut open).default_width(380.0).show(ctx, |ui| {
                let before = prefs.clone();
                Section::new("General").id_salt("prefs general").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(Toggle(&mut prefs.check_updates_at_startup));
                        ui.label("Check for updates at startup");
                    });
                });
                Section::new("Performance").id_salt("prefs performance").show(ui, |ui| {
                    ui.add(Field::new(&mut prefs.gpu_cache_budget_gb, 0.5..=128.0).label("GPU cache budget").decimals(1).suffix(" GB").speed(0.1).no_fill());
                    ui.add(Field::new(&mut prefs.undo_budget_gb, 0.1..=64.0).label("Undo budget").decimals(1).suffix(" GB").speed(0.1).no_fill());
                    ui.add(Field::new(&mut prefs.undo_steps, 1..=1000).label("Undo steps").speed(0.5).no_fill());
                });
                Section::new("Shortcuts").id_salt("prefs shortcuts").show(ui, |ui| {
                let bindings = crate::shortcuts::Bindings::new(&prefs.shortcuts);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("shortcuts").num_columns(2).show(ui, |ui| {
                        for a in crate::shortcuts::Action::ALL {
                            ui.label(a.label());
                            let text = if self.capture == Some(a) {
                                "Press keys…".to_string()
                            } else {
                                crate::shortcuts::format(&bindings.get(a))
                            };
                            let key = egui::RichText::new(text).font(crate::ui_theme::value()).color(crate::ui_theme::TEXT_2);
                            if ui.add(egui::Button::new(key).fill(crate::ui_theme::BG_INPUT)).clicked() {
                                self.capture = Some(a);
                            }
                            ui.end_row();
                        }
                    });
                });
                if ui.add(crate::ui_widgets::secondary("Reset to defaults")).clicked() {
                    prefs.shortcuts.clear();
                }
                });
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
                        ui.colored_label(crate::ui_theme::ATTENTION, "Over budget");
                        ui.end_row();
                    }
                });
                if let Some(e) = &p.cache_error {
                    ui.colored_label(crate::ui_theme::ERROR, e);
                }
                ui.separator();
                ui.strong("Cached nodes");
                if p.caches.entries.is_empty() {
                    ui.label(egui::RichText::new("None").weak());
                }
                egui::Grid::new("caches").num_columns(3).show(ui, |ui| {
                    for e in p.caches.entries.values() {
                        let name = p.graph.get(e.node).map(|n| n.title().to_string()).unwrap_or_default();
                        ui.label(name);
                        if e.signature.is_some() {
                            crate::ui_widgets::tag(ui, "ready", crate::ui_widgets::TagKind::Good);
                        } else {
                            crate::ui_widgets::tag(ui, "pending", crate::ui_widgets::TagKind::Attention);
                        }
                        ui.label(egui::RichText::new(format!("{:.1} ms", e.cost_ms)).font(crate::ui_theme::value()));
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
                ui.set_width(340.0);
                for c in Channel::ALL {
                    let i = c.index();
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(110.0, crate::ui_theme::H_CONTROL), egui::Sense::hover());
                        let mut on = ch.enabled[i];
                        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| ui.add(Check::new(&mut on, c.name())));
                        ch.enabled[i] = on;
                        ui.add_enabled_ui(ch.enabled[i], |ui| match c {
                            Channel::BaseColor | Channel::Emissive => {
                                ui.spacing_mut().interact_size = egui::vec2(ui.available_width(), crate::ui_theme::H_CONTROL - 4.0);
                                crate::ui_widgets::color_edit(ui, &mut ch.defaults[i]);
                            }
                            Channel::Normal => {
                                let fw = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(40.0);
                                for k in 0..3 {
                                    ui.add(Field::new(&mut ch.defaults[i][k], 0.0..=1.0).decimals(2).width(fw));
                                }
                            }
                            Channel::Height => {
                                ui.add(Field::new(&mut ch.defaults[i][0], -1.0..=1.0).label("Default"));
                            }
                            _ => {
                                ui.add(Field::new(&mut ch.defaults[i][0], 0.0..=1.0).label("Default"));
                            }
                        });
                    });
                }
                ui.add(Field::new(&mut ch.height_scale, 0.0001..=10.0).label("Height scale").decimals(4).suffix(" units").speed(0.0005).no_fill());
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
