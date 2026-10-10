//! Help > Check for Updates: the Updates window and the startup check, running
//! `crate::update` on a background thread. See docs/reference/updates.md.

use std::sync::{Arc, Mutex};

use crate::update::{self, Progress, Release};

#[derive(Clone, Debug)]
enum State {
    Idle,
    Checking,
    UpToDate(Release),
    Available(Release),
    /// Development builds see the latest release but don't install it.
    Development(Release),
    /// No archive for this platform.
    Unsupported(Release),
    Downloading(Release, u64, u64),
    Installing(Release),
    Installed(Release),
    Failed(String),
}

#[derive(Default)]
pub struct Updater {
    pub open: bool,
    state: Arc<Mutex<Option<State>>>,
}

impl Updater {
    fn state(&self) -> State {
        self.state.lock().unwrap().clone().unwrap_or(State::Idle)
    }

    fn set(state: &Arc<Mutex<Option<State>>>, ctx: &egui::Context, s: State) {
        *state.lock().unwrap() = Some(s);
        ctx.request_repaint();
    }

    fn busy(&self) -> bool {
        matches!(self.state(), State::Checking | State::Downloading(..) | State::Installing(_) | State::Installed(_))
    }

    /// Checks GitHub in the background. A `quiet` (startup) check reports nothing on failure.
    pub fn check(&self, ctx: &egui::Context, quiet: bool) {
        if self.busy() {
            return;
        }
        let (state, ctx) = (self.state.clone(), ctx.clone());
        Self::set(&state, &ctx, State::Checking);
        std::thread::spawn(move || {
            let s = match update::latest() {
                Ok(r) => match r.is_newer() {
                    None => State::Development(r),
                    Some(false) => State::UpToDate(r),
                    Some(true) if r.asset.is_none() => State::Unsupported(r),
                    Some(true) => State::Available(r),
                },
                Err(e) if quiet => {
                    log::info!("update check failed: {e:#}");
                    State::Idle
                }
                Err(e) => State::Failed(format!("{e:#}")),
            };
            Self::set(&state, &ctx, s);
        });
    }

    fn install(&self, ctx: &egui::Context, release: Release) {
        let (state, ctx) = (self.state.clone(), ctx.clone());
        Self::set(&state, &ctx, State::Downloading(release.clone(), 0, release.asset.as_ref().map_or(0, |a| a.size)));
        std::thread::spawn(move || {
            let r = release.clone();
            let progress = |p: Progress| {
                let s = match p {
                    Progress::Downloading { done, total } => State::Downloading(r.clone(), done, total),
                    Progress::Installing => State::Installing(r.clone()),
                };
                Self::set(&state, &ctx, s);
            };
            let s = match update::install(&release, &progress) {
                Ok(()) => State::Installed(release),
                Err(e) => State::Failed(format!("{e:#}")),
            };
            Self::set(&state, &ctx, s);
        });
    }

    /// The newer release, once a check found one that can be installed.
    pub fn available(&self) -> Option<String> {
        match self.state() {
            State::Available(r) => Some(r.display()),
            _ => None,
        }
    }

    /// Shows the window. Returns true when the user asks to restart into the installed version.
    pub fn ui(&mut self, ctx: &egui::Context) -> bool {
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut restart = false;
        let state = self.state();
        egui::Window::new("Updates").open(&mut open).resizable(false).default_width(420.0).show(ctx, |ui| {
            let release = match &state {
                State::UpToDate(r) | State::Available(r) | State::Development(r) | State::Unsupported(r) => Some(r),
                State::Downloading(r, ..) | State::Installing(r) | State::Installed(r) => Some(r),
                _ => None,
            };
            egui::Grid::new("update info").num_columns(2).show(ui, |ui| {
                ui.label("Installed");
                ui.label(update::version_str());
                ui.end_row();
                if let Some(r) = release {
                    ui.label("Latest");
                    ui.horizontal(|ui| {
                        ui.label(format!("{}, {}", r.display(), r.published));
                        if !r.html_url.is_empty() {
                            ui.hyperlink_to("View on GitHub", &r.html_url);
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(r) = release.filter(|r| !r.notes.is_empty()) {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Release notes").strong());
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    ui.label(&r.notes);
                });
            }
            ui.separator();
            let red = egui::Color32::from_rgb(230, 90, 80);
            let mut check = false;
            match &state {
                State::Idle => check = ui.button("Check now").clicked(),
                State::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking…");
                    });
                }
                State::UpToDate(_) => {
                    ui.label("Up to date.");
                    check = ui.button("Check again").clicked();
                }
                State::Available(r) => {
                    ui.label(format!("Crate {} is available.", r.display()));
                    ui.horizontal(|ui| {
                        if ui.button("Download and install").clicked() {
                            self.install(ctx, r.clone());
                        }
                        check = ui.button("Check again").clicked();
                    });
                }
                State::Development(_) => {
                    ui.label("This is a development build, built from source. It doesn't update itself.");
                    check = ui.button("Check again").clicked();
                }
                State::Unsupported(_) => {
                    ui.label("No release is published for this platform (only Windows x64 and Linux x64).");
                    check = ui.button("Check again").clicked();
                }
                State::Downloading(_, done, total) => {
                    let mb = |b: u64| b as f32 / (1024.0 * 1024.0);
                    let f = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(f).text(format!("Downloading {:.1} of {:.1} MB", mb(*done), mb(*total))));
                }
                State::Installing(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Installing…");
                    });
                }
                State::Installed(r) => {
                    ui.label(format!("Installed {}. Restart Crate to finish.", r.display()));
                    ui.horizontal(|ui| {
                        restart = ui.button("Restart now").clicked();
                        if ui.button("Later").clicked() {
                            self.open = false;
                        }
                    });
                }
                State::Failed(e) => {
                    ui.colored_label(red, format!("Failed: {e}"));
                    check = ui.button("Check again").clicked();
                }
            }
            if check {
                self.check(ctx, false);
            }
        });
        self.open &= open;
        restart
    }
}
