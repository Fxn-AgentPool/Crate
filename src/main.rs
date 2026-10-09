mod app;
mod bvh;
mod cache;
mod camera;
mod channels;
mod compile;
mod export;
mod gpu;
mod graph;
mod headless;
mod mesh;
mod meshmaps;
mod ops;
mod paint;
mod prefs;
mod project;
mod projectfile;
mod scene;
mod shortcuts;
mod smartmat;
mod script;
mod stencil;
mod thumbs;
mod ui_export;
mod ui_graph;
mod ui_layer_list;
mod ui_layers;
mod ui_paint;
mod ui_windows;
mod viewport;

use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,painter=info"))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--headless") {
        return headless::run(&args[1..]);
    }
    // painter [mesh] [--script startup.txt]
    let mut initial_mesh = None;
    let mut script = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--script" {
            script = it.next().map(std::path::PathBuf::from);
        } else {
            initial_mesh = Some(std::path::PathBuf::from(a));
        }
    }

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Painter")
            .with_inner_size([1600.0, 960.0]),
        ..Default::default()
    };
    if let egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.device_descriptor = Arc::new(gpu::device_descriptor);
    }
    eframe::run_native(
        "Painter",
        options,
        Box::new(move |cc| Ok(Box::new(app::PainterApp::new(cc, initial_mesh, script)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
