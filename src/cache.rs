//! Cached node evaluation and non-local filters.
//! See docs/explanation/about-realtime-evaluation.md.
//!
//! A cached node is evaluated in UV space once per tile into a texture, padded, optionally
//! filtered, and the material shader reads that texture instead of re-evaluating the
//! subgraph per pixel. Caches are recomputed only when their inputs change.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::compile::{self, CacheRef, Target};
use crate::gpu::{self, Gpu};
use crate::graph::{Link, NodeId, SocketType};
use crate::scene::PAINT_FORMAT;
use crate::viewport::vertex_layout;

#[derive(Clone, Debug)]
pub struct Entry {
    pub node: NodeId,
    pub cache: CacheRef,
    /// Signature of the inputs the cache was computed from; None until computed.
    pub signature: Option<u64>,
    pub cost_ms: f32,
}

#[derive(Default)]
pub struct Caches {
    pub entries: BTreeMap<NodeId, Entry>,
    next_value_slot: u32,
    free_value_slots: Vec<u32>,
    pipelines: HashMap<(u64, bool), wgpu::RenderPipeline>,
    filters: Option<Filters>,
    eval_layout: Option<wgpu::BindGroupLayout>,
}

struct Filters {
    pad: wgpu::ComputePipeline,
    blur: wgpu::ComputePipeline,
    dilate: wgpu::ComputePipeline,
    height_normal: wgpu::ComputePipeline,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FilterParams {
    res: u32,
    radius: f32,
    axis: u32,
    strength: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EvalParams {
    tile: [f32; 4],
}

impl Caches {
    /// What the compiler should read for `id`: only caches that have been computed.
    pub fn lookup(&self, id: NodeId) -> Option<CacheRef> {
        self.entries.get(&id).filter(|e| e.signature.is_some()).map(|e| e.cache)
    }

    /// GPU memory used by caches, in bytes.
    pub fn memory(&self, res: u32, tiles: u32) -> u64 {
        let per_tile = res as u64 * res as u64 * 8;
        self.entries
            .values()
            .map(|e| match e.cache {
                CacheRef::Value(_) => per_tile * tiles as u64,
                CacheRef::Material(_) => per_tile * tiles as u64 * crate::scene::PAINT_PLANES as u64,
            })
            .sum()
    }

    fn alloc_value_slot(&mut self) -> u32 {
        self.free_value_slots.pop().unwrap_or_else(|| {
            self.next_value_slot += 1;
            self.next_value_slot - 1
        })
    }
}

/// Inputs the cache of `node` depends on: the subgraph's structure and parameters, paint,
/// mesh maps and channel settings.
fn signature(p: &crate::project::Project, node: NodeId) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let sub = p.graph.subgraph(node);
    let mut has_paint = false;
    for id in &sub {
        let n = p.graph.node(*id);
        // Upstream caches contribute through their own signatures.
        if *id != node {
            if let Some(e) = p.caches.entries.get(id) {
                e.signature.hash(&mut h);
            }
        }
        serde_json::to_string(n).unwrap_or_default().hash(&mut h);
        has_paint |= matches!(n.ty.as_str(), "paint" | "paint_mask");
    }
    if has_paint {
        p.paint_epoch.hash(&mut h);
    }
    p.maps.version.hash(&mut h);
    p.scene.image_names.len().hash(&mut h);
    serde_json::to_string(&p.channels).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Brings caches up to date. Returns true if what the compiler should read changed (the
/// material must be recompiled).
pub fn update(gpu: &Gpu, p: &mut crate::project::Project) -> anyhow::Result<bool> {
    let Some(out) = p.graph.output_node() else { return Ok(false) };
    let live = p.graph.subgraph(out);
    // Wanted caches in dependency order (upstream first).
    let mut wanted: Vec<NodeId> = live
        .iter()
        .copied()
        .filter(|id| {
            let n = p.graph.node(*id);
            (n.def().non_local || n.cached) && compile::cacheable(n)
        })
        .collect();
    wanted.sort_by_key(|id| p.graph.subgraph(*id).len());

    let mut lookup_changed = false;
    // Drop caches that are no longer wanted.
    let stale: Vec<NodeId> = p.caches.entries.keys().copied().filter(|k| !wanted.contains(k)).collect();
    for k in stale {
        if let Some(e) = p.caches.entries.remove(&k) {
            lookup_changed |= e.signature.is_some();
            if let CacheRef::Value(s) = e.cache {
                p.caches.free_value_slots.push(s);
            }
        }
    }
    let res = p.tile_resolution();
    let tiles = p.tile_count();
    for id in wanted {
        if !p.caches.entries.contains_key(&id) {
            let material = p.graph.node(id).def().outputs.iter().any(|(_, t)| *t == SocketType::Material);
            let cache = if material {
                CacheRef::Material(p.alloc_material_slot(gpu))
            } else {
                let s = p.caches.alloc_value_slot();
                p.scene.ensure_cache_slots(gpu, s + 1, res, tiles);
                CacheRef::Value(s)
            };
            p.caches.entries.insert(id, Entry { node: id, cache, signature: None, cost_ms: 0.0 });
            // A grown pool must be rebound before anything samples it.
            p.update_scene(gpu);
        }
        let sig = signature(p, id);
        if p.caches.entries[&id].signature == Some(sig) {
            continue;
        }
        let t = std::time::Instant::now();
        compute(gpu, p, id)?;
        gpu.wait_idle();
        let e = p.caches.entries.get_mut(&id).unwrap();
        lookup_changed |= e.signature.is_none();
        e.signature = Some(sig);
        e.cost_ms = t.elapsed().as_secs_f32() * 1000.0;
    }
    Ok(lookup_changed)
}

fn eval_layout(gpu: &Gpu, c: &mut Caches) -> wgpu::BindGroupLayout {
    c.eval_layout
        .get_or_insert_with(|| {
            gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cache eval"),
                entries: &[gpu::uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
            })
        })
        .clone()
}

fn filters<'a>(gpu: &Gpu, c: &'a mut Caches) -> &'a Filters {
    c.filters.get_or_insert_with(|| {
        let d = &gpu.device;
        let module = gpu::shader(d, "cache filter", include_str!("shaders/cache_filter.wgsl"));
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
        Filters { pad: compute("pad"), blur: compute("blur"), dilate: compute("dilate"), height_normal: compute("height_normal") }
    })
}

/// Builds (or reuses) the evaluation pipeline for a target.
fn pipeline(gpu: &Gpu, p: &mut crate::project::Project, target: &Target, material: bool) -> anyhow::Result<wgpu::RenderPipeline> {
    let scene = &p.scene;
    let (slots, masks, caches) = (&p.paint_slots, &p.mask_slots, &p.caches);
    let env = compile::Env {
        preview: None,
        image_layer: &|n| scene.image_layer(n),
        paint_slot: &|id| slots.get(&id).or(masks.get(&id)).copied(),
        cache: &|id| caches.lookup(id),
    };
    let code = compile::compile_target(&p.graph, &p.channels, &env, target);
    let src = include_str!("shaders/cache_eval.wgsl")
        .replace("//{{SURFACE}}", crate::viewport::SURFACE_WGSL)
        .replace("//{{SCENE}}", crate::scene::WGSL)
        .replace("//{{LIB}}", compile::LIB_WGSL)
        .replace("//{{TARGET}}", &code);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    let key = (h.finish(), material);
    if let Some(pl) = p.caches.pipelines.get(&key) {
        return Ok(pl.clone());
    }
    let layout = eval_layout(gpu, &mut p.caches);
    let d = &gpu.device;
    let scope = d.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = gpu::shader(d, "cache eval", &src);
    let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("cache eval"),
        bind_group_layouts: &[Some(&layout), Some(&p.scene.layout)],
        immediate_size: 0,
    });
    let targets: Vec<Option<wgpu::ColorTargetState>> =
        (0..if material { 5 } else { 1 }).map(|_| Some(PAINT_FORMAT.into())).collect();
    let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cache eval"),
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
            entry_point: Some(if material { "fs_mat" } else { "fs_value" }),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    });
    if let Some(e) = pollster::block_on(scope.pop()) {
        anyhow::bail!("cache shader failed to compile: {e}");
    }
    if p.caches.pipelines.len() > 64 {
        p.caches.pipelines.clear();
    }
    p.caches.pipelines.insert(key, pipeline.clone());
    Ok(pipeline)
}

fn tex2d(gpu: &Gpu, res: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cache temp"),
        size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PAINT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn compute(gpu: &Gpu, p: &mut crate::project::Project, id: NodeId) -> anyhow::Result<()> {
    let node = p.graph.node(id).clone();
    let entry = p.caches.entries[&id].clone();
    let res = p.tile_resolution();
    let tiles = p.tile_count();
    let filter = node.def().non_local;
    let material = matches!(entry.cache, CacheRef::Material(_));
    let target = if filter {
        let want = node.def().inputs.first().map(|(_, t)| *t).unwrap_or(SocketType::Color);
        match node.inputs.get("in").cloned() {
            Some(link) => Target::Link(link, want),
            // Unconnected filter: evaluate a constant zero input.
            None => Target::Link(Link { node: id, output: "__none__".into() }, want),
        }
    } else {
        Target::Node(id)
    };
    let pipeline = pipeline(gpu, p, &target, material)?;
    let layout = eval_layout(gpu, &mut p.caches);
    let d = gpu.device.clone();
    let planes = if material { crate::scene::PAINT_PLANES } else { 1 };
    for tile in 0..tiles {
        let raw: Vec<wgpu::Texture> = (0..planes).map(|_| tex2d(gpu, res)).collect();
        let udim = p.mesh.tiles[tile as usize] - 1001;
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cache eval params"),
            contents: bytemuck::bytes_of(&EvalParams { tile: [(udim % 10) as f32, (udim / 10) as f32, tile as f32, 0.0] }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cache eval"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let views: Vec<wgpu::TextureView> = raw.iter().map(|t| t.create_view(&Default::default())).collect();
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let attachments: Vec<_> = views
                .iter()
                .map(|v| {
                    Some(wgpu::RenderPassColorAttachment {
                        view: v,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })
                })
                .collect();
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cache eval"),
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
        // Pad every plane, then filter.
        let source = p.maps.source.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: tile,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let pos = p.maps.position.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: tile,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let mut results = Vec::new();
        for r in &raw {
            let padded = tex2d(gpu, res);
            run_filter(gpu, &mut p.caches, &mut enc, Kind::Pad, r, &padded, &source, &pos, res, 0.0, 0, 1.0);
            results.push(padded);
        }
        if filter {
            let a = results.pop().unwrap();
            let b = tex2d(gpu, res);
            let radius = node.param("radius").as_f32();
            let out = match node.ty.as_str() {
                "blur" | "dilate" => {
                    let kind = if node.ty == "blur" { Kind::Blur } else { Kind::Dilate };
                    run_filter(gpu, &mut p.caches, &mut enc, kind, &a, &b, &source, &pos, res, radius, 0, 1.0);
                    let c = tex2d(gpu, res);
                    run_filter(gpu, &mut p.caches, &mut enc, kind, &b, &c, &source, &pos, res, radius, 1, 1.0);
                    c
                }
                _ => {
                    let strength = node.param("strength").as_f32();
                    run_filter(gpu, &mut p.caches, &mut enc, Kind::HeightNormal, &a, &b, &source, &pos, res, 0.0, 0, strength);
                    b
                }
            };
            results.push(out);
        }
        // Copy into the pool layer(s).
        let (pools, slot): (Vec<&wgpu::Texture>, u32) = match entry.cache {
            CacheRef::Value(s) => (vec![&p.scene.cache.texture], s),
            CacheRef::Material(s) => (p.scene.paint.iter().map(|pl| &pl.texture).collect(), s),
        };
        for (i, t) in results.iter().enumerate() {
            enc.copy_texture_to_texture(
                t.as_image_copy(),
                wgpu::TexelCopyTextureInfo {
                    texture: pools[i],
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: slot * tiles + tile },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
            );
        }
        gpu.queue.submit([enc.finish()]);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Pad,
    Blur,
    Dilate,
    HeightNormal,
}

#[allow(clippy::too_many_arguments)]
fn run_filter(
    gpu: &Gpu,
    c: &mut Caches,
    enc: &mut wgpu::CommandEncoder,
    kind: Kind,
    src: &wgpu::Texture,
    dst: &wgpu::Texture,
    source: &wgpu::TextureView,
    pos: &wgpu::TextureView,
    res: u32,
    radius: f32,
    axis: u32,
    strength: f32,
) {
    let f = filters(gpu, c);
    let pipe = match kind {
        Kind::Pad => &f.pad,
        Kind::Blur => &f.blur,
        Kind::Dilate => &f.dilate,
        Kind::HeightNormal => &f.height_normal,
    };
    let d = &gpu.device;
    let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("filter params"),
        contents: bytemuck::bytes_of(&FilterParams { res, radius, axis, strength }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let sv = src.create_view(&Default::default());
    let dv = dst.create_view(&Default::default());
    let mut entries = vec![
        wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&sv) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dv) },
    ];
    if kind == Kind::Pad {
        entries.push(wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(source) });
    }
    if kind == Kind::HeightNormal {
        entries.push(wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(pos) });
    }
    let bind = d.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("filter"), layout: &pipe.get_bind_group_layout(0), entries: &entries });
    let mut pass = enc.begin_compute_pass(&Default::default());
    pass.set_pipeline(pipe);
    pass.set_bind_group(0, &bind, &[]);
    pass.dispatch_workgroups(res.div_ceil(8), res.div_ceil(8), 1);
}
