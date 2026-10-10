//! Texture export. See docs/reference/export.md.
//!
//! Each tile is evaluated in UV space with the compiled material, height is converted to
//! normals and combined with the Normal channel, padding is filled by jump flooding, and
//! the result is packed into the preset's output maps.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

use crate::channels::Channel;
use crate::gpu::{self, Gpu};
use crate::project::Project;
use crate::viewport::vertex_layout;

pub const EVAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileFormat {
    Png,
    Tga,
    Exr,
}

impl FileFormat {
    pub fn ext(self) -> &'static str {
        match self {
            FileFormat::Png => "png",
            FileFormat::Tga => "tga",
            FileFormat::Exr => "exr",
        }
    }

    pub fn depths(self) -> &'static [u8] {
        match self {
            FileFormat::Png => &[8, 16],
            FileFormat::Tga => &[8],
            FileFormat::Exr => &[16, 32],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NormalFormat {
    OpenGl,
    DirectX,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Padding {
    None,
    Texels(u32),
    Infinite,
}

/// One output file per tile. Component sources:
/// `<channel>[.r|.g|.b]`, `normal.x|y|z` (combined normal), `ao`, `1-<source>`, `0`, `1`, "" = none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutputMap {
    pub name: String,
    pub sources: [String; 4],
    pub format: FileFormat,
    pub bits: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_format: Option<NormalFormat>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub maps: Vec<OutputMap>,
    /// Raw Channels: one EXR per enabled channel instead of `maps`.
    #[serde(default)]
    pub raw: bool,
}

fn map(name: &str, s: [&str; 4], format: FileFormat, bits: u8, nf: Option<NormalFormat>) -> OutputMap {
    OutputMap { name: name.into(), sources: s.map(String::from), format, bits, normal_format: nf }
}

pub fn builtin_presets() -> Vec<Preset> {
    use FileFormat::*;
    let base = map("BaseColor", ["base_color.r", "base_color.g", "base_color.b", "opacity"], Png, 8, None);
    let orm = map("ORM", ["ao", "roughness", "metallic", ""], Png, 8, None);
    let emissive = map("Emissive", ["emissive.r", "emissive.g", "emissive.b", ""], Png, 8, None);
    let normal = |nf| map("Normal", ["normal.x", "normal.y", "normal.z", ""], Png, 8, nf);
    vec![
        Preset { name: "glTF Metal/Rough".into(), maps: vec![base.clone(), orm.clone(), normal(None), emissive.clone()], raw: false },
        Preset {
            name: "Unreal Engine".into(),
            maps: vec![base.clone(), orm, normal(Some(NormalFormat::DirectX)), emissive.clone()],
            raw: false,
        },
        Preset {
            name: "Unity HDRP".into(),
            maps: vec![
                base,
                map("MaskMap", ["metallic", "ao", "1", "1-roughness"], Png, 8, None),
                normal(Some(NormalFormat::OpenGl)),
                emissive,
            ],
            raw: false,
        },
        Preset { name: "Raw Channels".into(), maps: vec![], raw: true },
    ]
}

/// Custom presets live in the user preferences directory.
pub fn presets_dir() -> PathBuf {
    crate::prefs::dir().join("export-presets")
}

pub fn all_presets() -> Vec<Preset> {
    let mut v = builtin_presets();
    if let Ok(entries) = std::fs::read_dir(presets_dir()) {
        for e in entries.flatten() {
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                if let Ok(p) = serde_json::from_str::<Preset>(&text) {
                    v.push(p);
                }
            }
        }
    }
    v
}

pub fn save_preset(p: &Preset) -> Result<PathBuf> {
    let dir = presets_dir();
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{}.json", p.name.replace(['/', '\\', ':'], "_")));
    std::fs::write(&file, serde_json::to_string_pretty(p)?)?;
    Ok(file)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportSettings {
    pub output_dir: PathBuf,
    pub preset: String,
    pub template: String,
    /// Overrides the preset's per-map format.
    pub format: Option<(FileFormat, u8)>,
    /// 0 = tile resolution.
    pub resolution: u32,
    pub padding: Padding,
    pub normal_format: NormalFormat,
    /// Empty = all tiles.
    pub tiles: Vec<u32>,
}

impl ExportSettings {
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            preset: "glTF Metal/Rough".into(),
            template: "{mesh}_{map}.{udim}.{ext}".into(),
            format: None,
            resolution: 0,
            padding: Padding::Infinite,
            normal_format: NormalFormat::OpenGl,
            tiles: Vec::new(),
        }
    }
}

pub struct Report {
    pub files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Channel(Channel, usize),
    Normal(usize),
    Ao,
    OneMinus(Box<Source>),
    Const(f32),
}

fn parse_source(s: &str) -> Result<Option<Source>> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if let Some(rest) = s.strip_prefix("1-") {
        return Ok(parse_source(rest)?.map(|x| Source::OneMinus(Box::new(x))));
    }
    if let Ok(v) = s.parse::<f32>() {
        return Ok(Some(Source::Const(v)));
    }
    if s == "ao" {
        return Ok(Some(Source::Ao));
    }
    let (name, comp) = s.split_once('.').unwrap_or((s, "r"));
    let comp = match comp {
        "r" | "x" => 0,
        "g" | "y" => 1,
        "b" | "z" => 2,
        _ => bail!("bad component in {s}"),
    };
    if name == "normal" {
        return Ok(Some(Source::Normal(comp)));
    }
    let c = Channel::ALL
        .into_iter()
        .find(|c| c.field() == name)
        .with_context(|| format!("unknown export source {s}"))?;
    Ok(Some(Source::Channel(c, comp)))
}

/// Channels a source reads (for the enabled/disabled rule).
fn source_channels(s: &Source, out: &mut Vec<Channel>) {
    match s {
        Source::Channel(c, _) => out.push(*c),
        Source::Normal(_) => {
            out.push(Channel::Normal);
            out.push(Channel::Height);
        }
        Source::OneMinus(x) => source_channels(x, out),
        _ => {}
    }
}

/// Evaluated tile data, f32 RGBA per plane: base+opacity, scalars, normal+ao, emissive.
struct TileData {
    res: u32,
    planes: [Vec<f32>; 4],
}

impl TileData {
    fn texel(&self, plane: usize, i: usize) -> [f32; 4] {
        let p = &self.planes[plane];
        [p[i * 4], p[i * 4 + 1], p[i * 4 + 2], p[i * 4 + 3]]
    }

    fn sample(&self, s: &Source, i: usize, ch: &crate::channels::ChannelSettings, nf: NormalFormat) -> f32 {
        match s {
            Source::Const(v) => *v,
            Source::Ao => self.texel(2, i)[3],
            Source::OneMinus(x) => 1.0 - self.sample(x, i, ch, nf),
            Source::Normal(k) => {
                let mut n = self.texel(2, i);
                if nf == NormalFormat::DirectX {
                    n[1] = -n[1];
                }
                n[*k] * 0.5 + 0.5
            }
            Source::Channel(c, k) => {
                if !ch.is_enabled(*c) {
                    let d = ch.defaults[c.index()];
                    return d[(*k).min(2)];
                }
                match c {
                    Channel::BaseColor => self.texel(0, i)[*k],
                    Channel::Opacity => self.texel(0, i)[3],
                    Channel::Roughness => self.texel(1, i)[0],
                    Channel::Metallic => self.texel(1, i)[1],
                    Channel::Height => self.texel(1, i)[2],
                    Channel::Normal => self.texel(2, i)[*k] * 0.5 + 0.5,
                    Channel::Emissive => self.texel(3, i)[*k],
                }
            }
        }
    }
}

fn is_color(s: &Source) -> bool {
    matches!(s, Source::Channel(Channel::BaseColor | Channel::Emissive, _))
}

fn srgb_encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EvalParams {
    tile: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FinishParams {
    res: u32,
    step: u32,
    limit: u32,
    _pad: u32,
    height: [f32; 4],
}

/// GPU evaluation of the material in UV space, shared by export and caches.
pub struct UvEvaluator {
    layout: wgpu::BindGroupLayout,
    pipeline: Option<(u64, wgpu::RenderPipeline)>,
    height_normal: wgpu::ComputePipeline,
    seed_init: wgpu::ComputePipeline,
    seed_step: wgpu::ComputePipeline,
    gather: wgpu::ComputePipeline,
}

impl UvEvaluator {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uv eval"),
            entries: &[gpu::uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let module = gpu::shader(d, "export finish", include_str!("shaders/export_finish.wgsl"));
        let compute = |entry: &str| {
            d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            layout,
            pipeline: None,
            height_normal: compute("height_normal"),
            seed_init: compute("seed_init"),
            seed_step: compute("seed_step"),
            gather: compute("gather"),
        }
    }

    fn pipeline(&mut self, gpu: &Gpu, p: &Project) -> Result<&wgpu::RenderPipeline> {
        if self.pipeline.as_ref().is_none_or(|(v, _)| *v != p.material_version) {
            let src = include_str!("shaders/export.wgsl")
                .replace("//{{SURFACE}}", crate::viewport::SURFACE_WGSL)
                .replace("//{{SCENE}}", crate::scene::WGSL)
                .replace("//{{MATERIAL}}", &p.material_code);
            let d = &gpu.device;
            let scope = d.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = gpu::shader(d, "uv eval", &src);
            let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("uv eval"),
                bind_group_layouts: &[Some(&self.layout), Some(&p.scene.layout)],
                immediate_size: 0,
            });
            let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("uv eval"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(vertex_layout())],
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(EVAL_FORMAT.into()), Some(EVAL_FORMAT.into()), Some(EVAL_FORMAT.into()), Some(EVAL_FORMAT.into())],
                }),
                multiview_mask: None,
                cache: None,
            });
            if let Some(e) = pollster::block_on(scope.pop()) {
                bail!("UV evaluation shader failed to compile: {e}");
            }
            self.pipeline = Some((p.material_version, pipeline));
        }
        Ok(&self.pipeline.as_ref().unwrap().1)
    }

    /// Evaluates one tile and returns the four padded planes as textures.
    pub fn evaluate(&mut self, gpu: &Gpu, p: &Project, layer: u32, padding: Padding) -> Result<Vec<wgpu::Texture>> {
        let res = p.tile_resolution();
        let d = gpu.device.clone();
        let tex = |label: &str, format, storage: bool| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | if storage { wgpu::TextureUsages::STORAGE_BINDING } else { wgpu::TextureUsages::RENDER_ATTACHMENT },
                view_formats: &[],
            })
        };
        let raw: Vec<_> = (0..4).map(|_| tex("uv eval", EVAL_FORMAT, false)).collect();
        let normal = tex("uv eval normal", EVAL_FORMAT, true);
        let out: Vec<_> = (0..4).map(|_| tex("uv eval out", EVAL_FORMAT, true)).collect();
        let seeds = [tex("seeds a", wgpu::TextureFormat::R32Uint, true), tex("seeds b", wgpu::TextureFormat::R32Uint, true)];
        let v = |t: &wgpu::Texture| t.create_view(&Default::default());
        let udim = p.mesh.tiles[layer as usize] - 1001;
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uv eval params"),
            contents: bytemuck::bytes_of(&EvalParams { tile: [(udim % 10) as f32, (udim / 10) as f32, layer as f32, 0.0] }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uv eval"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let pipeline = self.pipeline(gpu, p)?.clone();
        let raw_views: Vec<_> = raw.iter().map(v).collect();
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let attachments: Vec<_> = raw_views
                .iter()
                .map(|view| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })
                })
                .collect();
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("uv eval"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_bind_group(1, p.scene.bind_group.as_ref().unwrap(), &[]);
            pass.set_vertex_buffer(0, p.gpu_mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(p.gpu_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..p.gpu_mesh.index_count, 0, 0..1);
        }

        let limit = match padding {
            Padding::None => 0,
            Padding::Texels(n) => n,
            Padding::Infinite => u32::MAX,
        };
        let finish = |step: u32| {
            d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("finish params"),
                contents: bytemuck::bytes_of(&FinishParams {
                    res,
                    step,
                    limit,
                    _pad: 0,
                    height: [p.channels.height_scale, p.maps.res as f32, 0.0, 0.0],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };
        let groups = res.div_ceil(8);
        let run = |enc: &mut wgpu::CommandEncoder, pipe: &wgpu::ComputePipeline, entries: &[wgpu::BindGroupEntry]| {
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("finish"),
                layout: &pipe.get_bind_group_layout(0),
                entries,
            });
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups, groups, 1);
        };
        // Height to normal.
        let pos_layer = p.maps.position.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let fp = finish(0);
        let normal_view = v(&normal);
        run(&mut enc, &self.height_normal, &[
            wgpu::BindGroupEntry { binding: 0, resource: fp.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: tv(&raw_views[1]) },
            wgpu::BindGroupEntry { binding: 2, resource: tv(&raw_views[2]) },
            wgpu::BindGroupEntry { binding: 3, resource: tv(&pos_layer) },
            wgpu::BindGroupEntry { binding: 4, resource: tv(&normal_view) },
        ]);
        // Jump flood.
        let seed_views = [v(&seeds[0]), v(&seeds[1])];
        run(&mut enc, &self.seed_init, &[
            wgpu::BindGroupEntry { binding: 0, resource: fp.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: tv(&raw_views[1]) },
            wgpu::BindGroupEntry { binding: 6, resource: tv(&seed_views[0]) },
        ]);
        let mut cur = 0;
        let mut step = res.next_power_of_two() / 2;
        let mut step_bufs = Vec::new();
        if limit > 0 {
            while step >= 1 {
                let sp = finish(step);
                run(&mut enc, &self.seed_step, &[
                    wgpu::BindGroupEntry { binding: 0, resource: sp.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: tv(&seed_views[cur]) },
                    wgpu::BindGroupEntry { binding: 6, resource: tv(&seed_views[1 - cur]) },
                ]);
                step_bufs.push(sp);
                cur = 1 - cur;
                step /= 2;
            }
        }
        let out_views: Vec<_> = out.iter().map(v).collect();
        run(&mut enc, &self.gather, &[
            wgpu::BindGroupEntry { binding: 0, resource: fp.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: tv(&seed_views[cur]) },
            wgpu::BindGroupEntry { binding: 7, resource: tv(&raw_views[0]) },
            wgpu::BindGroupEntry { binding: 8, resource: tv(&raw_views[1]) },
            wgpu::BindGroupEntry { binding: 9, resource: tv(&normal_view) },
            wgpu::BindGroupEntry { binding: 10, resource: tv(&raw_views[3]) },
            wgpu::BindGroupEntry { binding: 11, resource: tv(&out_views[0]) },
            wgpu::BindGroupEntry { binding: 12, resource: tv(&out_views[1]) },
            wgpu::BindGroupEntry { binding: 13, resource: tv(&out_views[2]) },
            wgpu::BindGroupEntry { binding: 14, resource: tv(&out_views[3]) },
        ]);
        gpu.queue.submit([enc.finish()]);
        Ok(out)
    }
}

fn read_plane(gpu: &Gpu, t: &wgpu::Texture) -> Vec<f32> {
    let bytes = gpu.read_texture(t, 0, 8);
    bytemuck::cast_slice::<u8, half::f16>(&bytes).iter().map(|h| h.to_f32()).collect()
}

/// Box-downsamples RGBA f32 data by an integer factor.
fn downsample(data: &[f32], res: u32, factor: u32) -> Vec<f32> {
    if factor <= 1 {
        return data.to_vec();
    }
    let out_res = res / factor;
    let mut out = vec![0f32; (out_res * out_res * 4) as usize];
    let norm = 1.0 / (factor * factor) as f32;
    for y in 0..out_res {
        for x in 0..out_res {
            for c in 0..4 {
                let mut sum = 0.0;
                for dy in 0..factor {
                    for dx in 0..factor {
                        let i = ((y * factor + dy) * res + x * factor + dx) * 4 + c;
                        sum += data[i as usize];
                    }
                }
                out[((y * out_res + x) * 4 + c) as usize] = sum * norm;
            }
        }
    }
    out
}

pub fn file_name(template: &str, mesh: &str, project: &str, map: &str, udim: u32, ext: &str) -> String {
    template
        .replace("{mesh}", mesh)
        .replace("{project}", project)
        .replace("{map}", map)
        .replace("{udim}", &udim.to_string())
        .replace("{ext}", ext)
}

pub fn export(gpu: &Gpu, p: &mut Project, eval: &mut UvEvaluator, s: &ExportSettings) -> Result<Report> {
    p.finish_background_work(gpu);
    let presets = all_presets();
    let preset = presets.iter().find(|x| x.name == s.preset).with_context(|| format!("unknown preset {}", s.preset))?.clone();
    std::fs::create_dir_all(&s.output_dir).with_context(|| format!("creating {}", s.output_dir.display()))?;
    let mesh_name = p.original_mesh.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let project_name = p.name.clone().unwrap_or_else(|| mesh_name.clone());
    let res = p.tile_resolution();
    let out_res = if s.resolution == 0 || s.resolution >= res { res } else { s.resolution.next_power_of_two().min(res) };
    let factor = res / out_res;

    // Resolve the maps to write and the warnings.
    let mut warnings = Vec::new();
    let mut maps: Vec<(OutputMap, [Option<Source>; 4])> = Vec::new();
    if preset.raw {
        for c in Channel::ALL {
            if !p.channels.is_enabled(c) {
                continue;
            }
            let field = c.field();
            let sources = if c.components() == 3 {
                [format!("{field}.r"), format!("{field}.g"), format!("{field}.b"), String::new()]
            } else {
                [field.to_string(), String::new(), String::new(), String::new()]
            };
            let m = OutputMap {
                name: c.name().replace(' ', ""),
                sources,
                format: FileFormat::Exr,
                bits: 16,
                normal_format: None,
            };
            let parsed = [0, 1, 2, 3].map(|k| parse_source(&m.sources[k]).ok().flatten());
            maps.push((m, parsed));
        }
    } else {
        for m in &preset.maps {
            let mut parsed: [Option<Source>; 4] = Default::default();
            for k in 0..4 {
                parsed[k] = parse_source(&m.sources[k])?;
            }
            let mut chans = Vec::new();
            for src in parsed.iter().flatten() {
                source_channels(src, &mut chans);
            }
            let enabled = chans.iter().filter(|c| p.channels.is_enabled(**c)).count();
            if !chans.is_empty() && enabled == 0 {
                continue; // Every channel it reads is disabled: skip the map.
            }
            // A disabled channel feeding only the alpha component (usually Opacity) exports
            // as its default silently; one feeding color components is worth a warning.
            let mut rgb_chans = Vec::new();
            for src in parsed[..3].iter().flatten() {
                source_channels(src, &mut rgb_chans);
            }
            let disabled: Vec<&str> = rgb_chans
                .iter()
                .filter(|c| !p.channels.is_enabled(**c) && **c != Channel::Height)
                .map(|c| c.name())
                .collect();
            if !disabled.is_empty() {
                warnings.push(format!("{}: {} disabled, exported as default value", m.name, disabled.join(", ")));
            }
            maps.push((m.clone(), parsed));
        }
    }

    let tiles: Vec<(u32, u32)> = p
        .mesh
        .tiles
        .iter()
        .enumerate()
        .filter(|(_, u)| s.tiles.is_empty() || s.tiles.contains(u))
        .map(|(i, u)| (i as u32, *u))
        .collect();
    let mut files = Vec::new();
    for (layer, udim) in tiles {
        let planes = eval.evaluate(gpu, p, layer, s.padding)?;
        let data: Vec<Vec<f32>> = planes.iter().map(|t| downsample(&read_plane(gpu, t), res, factor)).collect();
        let tile = TileData { res: out_res, planes: [data[0].clone(), data[1].clone(), data[2].clone(), data[3].clone()] };
        for (m, sources) in &maps {
            let (format, bits) = if preset.raw { (m.format, m.bits) } else { s.format.unwrap_or((m.format, m.bits)) };
            let nf = m.normal_format.unwrap_or(s.normal_format);
            let channels = if sources[3].is_some() { 4 } else if sources[1].is_some() { 3 } else { 1 };
            let path = s.output_dir.join(file_name(&s.template, &mesh_name, &project_name, &m.name, udim, format.ext()));
            write_map(&path, &tile, sources, channels, format, bits, nf, &p.channels)?;
            files.push(path);
        }
    }
    Ok(Report { files, warnings })
}

#[allow(clippy::too_many_arguments)]
fn write_map(
    path: &Path,
    tile: &TileData,
    sources: &[Option<Source>; 4],
    channels: usize,
    format: FileFormat,
    bits: u8,
    nf: NormalFormat,
    ch: &crate::channels::ChannelSettings,
) -> Result<()> {
    let n = (tile.res * tile.res) as usize;
    let value = |k: usize, i: usize| -> f32 {
        match &sources[k] {
            Some(src) => tile.sample(src, i, ch, nf),
            None => if k == 3 { 1.0 } else { 0.0 },
        }
    };
    let res = tile.res;
    match format {
        FileFormat::Exr => {
            let get = |x: usize, y: usize, k: usize| value(k, y * res as usize + x);
            let r = if bits == 32 {
                match channels {
                    1 => exr::prelude::write_rgb_file(path, res as usize, res as usize, |x, y| {
                        let v = get(x, y, 0);
                        (v, v, v)
                    }),
                    3 => exr::prelude::write_rgb_file(path, res as usize, res as usize, |x, y| (get(x, y, 0), get(x, y, 1), get(x, y, 2))),
                    _ => exr::prelude::write_rgba_file(path, res as usize, res as usize, |x, y| {
                        (get(x, y, 0), get(x, y, 1), get(x, y, 2), get(x, y, 3))
                    }),
                }
            } else {
                let h = half::f16::from_f32;
                match channels {
                    1 => exr::prelude::write_rgb_file(path, res as usize, res as usize, |x, y| {
                        let v = h(get(x, y, 0));
                        (v, v, v)
                    }),
                    3 => exr::prelude::write_rgb_file(path, res as usize, res as usize, |x, y| {
                        (h(get(x, y, 0)), h(get(x, y, 1)), h(get(x, y, 2)))
                    }),
                    _ => exr::prelude::write_rgba_file(path, res as usize, res as usize, |x, y| {
                        (h(get(x, y, 0)), h(get(x, y, 1)), h(get(x, y, 2)), h(get(x, y, 3)))
                    }),
                }
            };
            r.map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))?;
        }
        FileFormat::Png | FileFormat::Tga => {
            let enc = |k: usize, i: usize| -> f32 {
                let v = value(k, i);
                if sources[k].as_ref().is_some_and(is_color) { srgb_encode(v) } else { v.clamp(0.0, 1.0) }
            };
            let color = match channels {
                1 => image::ColorType::L8,
                3 => image::ColorType::Rgb8,
                _ => image::ColorType::Rgba8,
            };
            let comps = match channels {
                1 => 1,
                3 => 3,
                _ => 4,
            };
            if bits == 16 && format == FileFormat::Png {
                let mut buf = Vec::with_capacity(n * comps);
                for i in 0..n {
                    for k in 0..comps {
                        buf.push((enc(k, i) * 65535.0 + 0.5) as u16);
                    }
                }
                let color = match comps {
                    1 => image::ColorType::L16,
                    3 => image::ColorType::Rgb16,
                    _ => image::ColorType::Rgba16,
                };
                image::save_buffer(path, bytemuck::cast_slice(&buf), res, res, color)?;
            } else {
                let mut buf = Vec::with_capacity(n * comps);
                for i in 0..n {
                    for k in 0..comps {
                        buf.push((enc(k, i) * 255.0 + 0.5) as u8);
                    }
                }
                image::save_buffer(path, &buf, res, res, color)?;
            }
        }
    }
    Ok(())
}

fn tv(v: &wgpu::TextureView) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::TextureView(v)
}
