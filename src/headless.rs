//! Headless commands, used for verification and scripted runs.
//!
//! crate --headless render <mesh> <out.png> [--size WxH] [--yaw deg] [--pitch deg]
//!         [--tiles] [--wire] [--up z] [--bake] [--show map|channel] [--res N]

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::camera::Camera;
use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::mesh::{self, ImportSettings, UpAxis};
use crate::meshmaps::MeshMap;
use crate::project::Project;
use crate::viewport::{DisplayMode, ViewSettings, Viewport};

pub fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("render") => render(&args[1..]),
        Some("run") => {
            let script = PathBuf::from(args.get(1).context("run <script.txt>")?);
            let mut session = crate::script::Session::new(std::env::current_dir()?)?;
            session.run_file(&script)
        }
        _ => bail!("usage: crate --headless render <mesh> <out.png> [options] | run <script.txt>"),
    }
}

struct Opts {
    positional: Vec<String>,
    size: (u32, u32),
    yaw: Option<f32>,
    pitch: Option<f32>,
    tiles: bool,
    wire: bool,
    up_z: bool,
    bake: bool,
    show: Option<String>,
    res: u32,
}

fn parse(args: &[String]) -> Result<Opts> {
    let mut o = Opts {
        positional: Vec::new(),
        size: (960, 720),
        yaw: None,
        pitch: None,
        tiles: false,
        wire: false,
        up_z: false,
        bake: false,
        show: None,
        res: 2048,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().context("missing option value");
        match a.as_str() {
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').context("--size WxH")?;
                o.size = (w.parse()?, h.parse()?);
            }
            "--yaw" => o.yaw = Some(val()?.parse::<f32>()?.to_radians()),
            "--pitch" => o.pitch = Some(val()?.parse::<f32>()?.to_radians()),
            "--tiles" => o.tiles = true,
            "--wire" => o.wire = true,
            "--up" => o.up_z = val()? == "z",
            "--bake" => o.bake = true,
            "--show" => o.show = Some(val()?.clone()),
            "--res" => o.res = val()?.parse()?,
            _ => o.positional.push(a.clone()),
        }
    }
    Ok(o)
}

fn display_mode(name: &str) -> Result<DisplayMode> {
    let n = name.to_ascii_lowercase().replace([' ', '_', '-'], "");
    // Channels first: "normal" and "height" are channels; the maps are "normal-map", "height-map".
    for (i, c) in Channel::ALL.iter().enumerate() {
        if c.name().to_ascii_lowercase().replace(' ', "") == n {
            return Ok(DisplayMode::Channel(i as u32));
        }
    }
    for (i, m) in MeshMap::ALL.iter().enumerate() {
        let name = m.name().to_ascii_lowercase().replace(' ', "");
        if name == n || m.file_stem().replace('_', "") == n || format!("{name}map") == n {
            return Ok(DisplayMode::MeshMap(i as u32));
        }
    }
    match n.as_str() {
        "ao" => Ok(DisplayMode::MeshMap(MeshMap::AmbientOcclusion.index() as u32)),
        "material" => Ok(DisplayMode::Material),
        _ => bail!("unknown map or channel: {name}"),
    }
}

fn render(args: &[String]) -> Result<()> {
    let o = parse(args)?;
    let [mesh_path, out_path] = o.positional.as_slice() else {
        bail!("render needs <mesh> <out.png>");
    };
    let settings = ImportSettings {
        up_axis: if o.up_z { UpAxis::Z } else { UpAxis::Y },
        tile_resolution: o.res,
        ..Default::default()
    };
    let mesh = mesh::import(&PathBuf::from(mesh_path), &settings)?;
    println!(
        "Imported {}: {} vertices, {} triangles, materials {:?}, tiles {:?}",
        mesh_path,
        mesh.vertices.len(),
        mesh.triangle_count(),
        mesh.materials,
        mesh.tiles
    );
    for d in &mesh.diagnostics {
        println!("  [{:?}] {}: {}", d.severity, d.code, d.message);
    }
    if mesh.has_errors() {
        bail!("Import aborted because of errors.");
    }

    let gpu = Gpu::headless()?;
    let mut project = Project::new(&gpu, mesh, settings);
    if o.bake {
        let t = std::time::Instant::now();
        project.start_bake(&gpu, &[MeshMap::AmbientOcclusion, MeshMap::Curvature, MeshMap::Thickness, MeshMap::Id]);
        project.finish_background_work(&gpu);
        println!("Baked mesh maps in {:.2}s", t.elapsed().as_secs_f32());
    }

    let mut viewport = Viewport::new(&gpu, &project.scene.layout);
    viewport.set_mesh(&gpu, &project.gpu_mesh);
    let mut camera = Camera::default();
    if let Some(y) = o.yaw {
        camera.yaw = y;
    }
    if let Some(p) = o.pitch {
        camera.pitch = p;
    }
    camera.frame(project.mesh.center(), project.mesh.radius());
    let display = match &o.show {
        Some(s) => display_mode(s)?,
        None => DisplayMode::Material,
    };
    let view = ViewSettings { show_tiles: o.tiles, wireframe: o.wire, display, ..Default::default() };
    viewport.render(
        &gpu,
        &camera,
        &project.mesh,
        &project.gpu_mesh,
        project.scene.bind_group.as_ref().unwrap(),
        &view,
        o.size.0,
        o.size.1,
    );
    let pixels = gpu.read_texture(viewport.output_texture().unwrap(), 0, 4);
    image::save_buffer(out_path, &pixels, o.size.0, o.size.1, image::ColorType::Rgba8)?;
    println!("Wrote {out_path}");
    Ok(())
}
