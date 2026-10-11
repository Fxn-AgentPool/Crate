mod app;
mod bakepreview;
mod bakeview;
mod bvh;
mod cache;
mod cage;
mod camera;
mod channels;
mod cli;
mod compile;
mod export;
mod gpu;
mod graph;
mod headless;
mod library;
mod highpoly;
mod mesh;
mod meshmaps;
mod ops;
mod paint;
mod prefs;
mod project;
mod projectfile;
mod recent;
mod scene;
mod shortcuts;
mod smartmat;
mod script;
mod stencil;
mod thumbs;
mod ui_library;
mod ui_bake_preview;
mod ui_export;
mod ui_graph;
mod ui_layer_list;
mod ui_layers;
mod ui_paint;
mod ui_theme;
mod ui_update;
mod ui_widgets;
mod ui_windows;
mod update;
mod viewport;

use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;

fn init_logging(default: &str) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default)).init();
}

fn main() -> ExitCode {
    // Removes the program the last update replaced (docs/reference/updates.md).
    update::cleanup_previous();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Older form used by the test scripts: crate --headless run|render ...
    if args.first().map(String::as_str) == Some("--headless") {
        init_logging("error");
        return match headless::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error: {e:?}");
                ExitCode::from(cli::EXIT_FAILED)
            }
        };
    }
    let cli = cli::Cli::parse();
    if cli.command.is_some() {
        init_logging(match cli.verbose {
            0 => "error",
            1 => "error,crate_app=info",
            _ => "warn,crate_app=debug",
        });
        return ExitCode::from(cli::main(cli));
    }

    init_logging("warn,crate_app=info");
    let initial = cli.path.clone();
    let script = cli.script.clone();
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Crate")
            .with_inner_size([1600.0, 960.0]),
        ..Default::default()
    };
    if let egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.device_descriptor = Arc::new(gpu::device_descriptor);
    }
    match eframe::run_native(
        "Crate",
        options,
        Box::new(move |cc| Ok(Box::new(app::CrateApp::new(cc, initial, script)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(cli::EXIT_FAILED)
        }
    }
}
