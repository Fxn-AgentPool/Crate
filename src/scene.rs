//! Bind group 1: everything the compiled material shader can read. The layout is fixed;
//! the material compiler only changes shader code and the parameter buffer, never bindings.
//!
//! Pools are texture arrays whose layer is `slot * tile_count + tile_layer`.

use bytemuck::{Pod, Zeroable};

use crate::gpu::{self, Gpu};
use crate::meshmaps::{MapStatus, MeshMap, MeshMaps};

pub const PAINT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Paint channel data is split over five textures (40 bytes per texel):
/// 0: base color rgb, coverage | 1: emissive rgb, coverage | 2: normal xyz, coverage |
/// 3: roughness, metallic, height, opacity | 4: coverage of roughness, metallic, height, opacity
pub const PAINT_PLANES: usize = 5;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct SceneUniform {
    pub bbox_min: [f32; 4],
    pub bbox_size: [f32; 4],
    /// x: bit per mesh map that has been baked, y: tile count, z: map resolution
    pub info: [u32; 4],
}

/// A growable texture array. Layer `slot * tiles + tile` holds one slot's tile.
pub struct Pool {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub res: u32,
    pub layers: u32,
}

impl Pool {
    pub fn new(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, res: u32, layers: u32) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: layers.max(1) },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self { texture, view, res, layers: layers.max(1) }
    }

    pub fn layer_view(&self, layer: u32) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer,
            array_layer_count: Some(1),
            ..Default::default()
        })
    }
}

pub fn layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
    let fs = wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
    let arr = wgpu::TextureViewDimension::D2Array;
    let mut entries = vec![
        gpu::sampler_entry(0, fs),
        gpu::sampler_entry(1, fs),
    ];
    // 2..=7 mesh maps: position, normal, id, ao, curvature, thickness
    for b in 2..=7 {
        entries.push(gpu::texture_entry(b, fs, arr, true));
    }
    entries.push(gpu::uniform_entry(8, fs));
    // 9..=13 paint planes, 14 mask paint, 15 images, 16 cache
    for b in 9..=16 {
        entries.push(gpu::texture_entry(b, fs, arr, true));
    }
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 17,
        visibility: fs,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene"),
        entries: &entries,
    })
}

/// WGSL declarations matching `layout`, shared by every shader that evaluates materials.
pub const WGSL: &str = r#"
struct Scene {
    bbox_min: vec4f,
    bbox_size: vec4f,
    info: vec4u,
};

@group(1) @binding(0) var samp_clamp: sampler;
@group(1) @binding(1) var samp_repeat: sampler;
@group(1) @binding(2) var mm_pos: texture_2d_array<f32>;
@group(1) @binding(3) var mm_nrm: texture_2d_array<f32>;
@group(1) @binding(4) var mm_id: texture_2d_array<f32>;
@group(1) @binding(5) var mm_ao: texture_2d_array<f32>;
@group(1) @binding(6) var mm_curv: texture_2d_array<f32>;
@group(1) @binding(7) var mm_thick: texture_2d_array<f32>;
@group(1) @binding(8) var<uniform> scene: Scene;
@group(1) @binding(9) var paint0: texture_2d_array<f32>;
@group(1) @binding(10) var paint1: texture_2d_array<f32>;
@group(1) @binding(11) var paint2: texture_2d_array<f32>;
@group(1) @binding(12) var paint3: texture_2d_array<f32>;
@group(1) @binding(13) var paint4: texture_2d_array<f32>;
@group(1) @binding(14) var mask_paint: texture_2d_array<f32>;
@group(1) @binding(15) var images: texture_2d_array<f32>;
@group(1) @binding(16) var cache: texture_2d_array<f32>;
@group(1) @binding(17) var<storage, read> params: array<vec4f>;

// Texture coordinate of a point within its tile (row 0 is the top of the tile).
fn tile_tex(tile_uv: vec2f) -> vec2f {
    return vec2f(tile_uv.x, 1.0 - tile_uv.y);
}

fn map_baked(i: u32) -> bool {
    return (scene.info.x & (1u << i)) != 0u;
}

// Mesh maps, with neutral values when not baked.
fn mesh_ao(tc: vec2f, layer: i32) -> f32 {
    if !map_baked(2u) || layer < 0 { return 1.0; }
    return textureSampleLevel(mm_ao, samp_clamp, tc, layer, 0.0).r;
}
fn mesh_curvature(tc: vec2f, layer: i32) -> f32 {
    if !map_baked(3u) || layer < 0 { return 0.5; }
    return textureSampleLevel(mm_curv, samp_clamp, tc, layer, 0.0).r;
}
fn mesh_thickness(tc: vec2f, layer: i32) -> f32 {
    if !map_baked(4u) || layer < 0 { return 1.0; }
    return textureSampleLevel(mm_thick, samp_clamp, tc, layer, 0.0).r;
}
fn mesh_id(tc: vec2f, layer: i32) -> vec3f {
    if !map_baked(5u) || layer < 0 { return vec3f(0.0); }
    // Nearest: ID colors must not blend at material borders.
    let res = textureDimensions(mm_id).x;
    let p = vec2u(clamp(tc * f32(res), vec2f(0.0), vec2f(f32(res) - 1.0)));
    return textureLoad(mm_id, p, layer, 0).rgb;
}
fn mesh_position(tc: vec2f, layer: i32) -> vec3f {
    if !map_baked(0u) || layer < 0 { return vec3f(0.0); }
    let p = textureSampleLevel(mm_pos, samp_clamp, tc, layer, 0.0).xyz;
    return (p - scene.bbox_min.xyz) / scene.bbox_size.xyz;
}
fn mesh_world_normal(tc: vec2f, layer: i32) -> vec3f {
    if !map_baked(1u) || layer < 0 { return vec3f(0.5, 0.5, 1.0); }
    return textureSampleLevel(mm_nrm, samp_clamp, tc, layer, 0.0).xyz;
}
"#;

pub struct SceneResources {
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: Option<wgpu::BindGroup>,
    uniform: wgpu::Buffer,
    sampler_clamp: wgpu::Sampler,
    sampler_repeat: wgpu::Sampler,
    pub paint: Vec<Pool>,
    pub mask: Pool,
    pub images: Pool,
    pub cache: Pool,
    pub params: wgpu::Buffer,
    pub params_capacity: usize,
    /// Names of images in the image pool, by layer.
    pub image_names: Vec<String>,
    dirty: bool,
    bound_maps_version: u64,
}

/// Images are resampled to this square size in the image pool.
pub const IMAGE_RES: u32 = 2048;

impl SceneResources {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let sampler = |mode| {
            d.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("scene sampler"),
                address_mode_u: mode,
                address_mode_v: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        let dummy = |label| Pool::new(gpu, label, PAINT_FORMAT, 1, 1);
        Self {
            layout: layout(gpu),
            bind_group: None,
            uniform: d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene uniform"),
                size: std::mem::size_of::<SceneUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler_clamp: sampler(wgpu::AddressMode::ClampToEdge),
            sampler_repeat: sampler(wgpu::AddressMode::Repeat),
            paint: (0..PAINT_PLANES).map(|_| dummy("paint plane")).collect(),
            mask: dummy("mask pool"),
            images: Pool::new(gpu, "image pool", IMAGE_FORMAT, 1, 1),
            cache: dummy("cache pool"),
            params: d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("node params"),
                size: 16 * 256,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            params_capacity: 256,
            image_names: Vec::new(),
            dirty: true,
            bound_maps_version: u64::MAX,
        }
    }

    pub fn image_layer(&self, name: &str) -> i32 {
        if name.is_empty() {
            return -1;
        }
        self.image_names.iter().position(|n| n == name).map(|i| i as i32).unwrap_or(-1)
    }

    /// Adds (or replaces) an image in the image pool. `rgba` is IMAGE_RES² RGBA8.
    pub fn set_image(&mut self, gpu: &Gpu, name: &str, rgba: &[u8]) -> u32 {
        let layer = match self.image_names.iter().position(|n| n == name) {
            Some(i) => i as u32,
            None => {
                self.image_names.push(name.to_string());
                let needed = self.image_names.len() as u32;
                if self.images.res != IMAGE_RES || self.images.layers < needed {
                    let old = std::mem::replace(
                        &mut self.images,
                        Pool::new(gpu, "image pool", IMAGE_FORMAT, IMAGE_RES, needed.next_power_of_two()),
                    );
                    if old.res == IMAGE_RES {
                        let mut enc = gpu.device.create_command_encoder(&Default::default());
                        enc.copy_texture_to_texture(
                            old.texture.as_image_copy(),
                            self.images.texture.as_image_copy(),
                            wgpu::Extent3d { width: IMAGE_RES, height: IMAGE_RES, depth_or_array_layers: old.layers },
                        );
                        gpu.queue.submit([enc.finish()]);
                    }
                    self.dirty = true;
                }
                needed - 1
            }
        };
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.images.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(IMAGE_RES * 4), rows_per_image: Some(IMAGE_RES) },
            wgpu::Extent3d { width: IMAGE_RES, height: IMAGE_RES, depth_or_array_layers: 1 },
        );
        layer
    }

    /// Grows a pool to at least `layers` layers at `res`, keeping existing contents.
    fn grow(gpu: &Gpu, pool: &mut Pool, label: &str, res: u32, layers: u32) -> bool {
        if pool.res == res && pool.layers >= layers {
            return false;
        }
        let new = Pool::new(gpu, label, PAINT_FORMAT, res, layers.next_power_of_two());
        if pool.res == res {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_texture(
                pool.texture.as_image_copy(),
                new.texture.as_image_copy(),
                wgpu::Extent3d { width: res, height: res, depth_or_array_layers: pool.layers },
            );
            gpu.queue.submit([enc.finish()]);
        }
        *pool = new;
        true
    }

    /// Makes room for `slots` material paint slots of `tiles` tiles each.
    pub fn ensure_paint_slots(&mut self, gpu: &Gpu, slots: u32, res: u32, tiles: u32) {
        let mut grew = false;
        for (i, plane) in self.paint.iter_mut().enumerate() {
            grew |= Self::grow(gpu, plane, &format!("paint plane {i}"), res, slots * tiles);
        }
        self.dirty |= grew;
    }

    pub fn ensure_cache_slots(&mut self, gpu: &Gpu, slots: u32, res: u32, tiles: u32) {
        self.dirty |= Self::grow(gpu, &mut self.cache, "cache pool", res, slots * tiles);
    }

    pub fn ensure_mask_slots(&mut self, gpu: &Gpu, slots: u32, res: u32, tiles: u32) {
        self.dirty |= Self::grow(gpu, &mut self.mask, "mask pool", res, slots * tiles);
    }

    /// Call after replacing a pool or the params buffer.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn write_params(&mut self, gpu: &Gpu, values: &[[f32; 4]]) {
        if values.len() > self.params_capacity {
            self.params_capacity = values.len().next_power_of_two();
            self.params = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("node params"),
                size: 16 * self.params_capacity as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.dirty = true;
        }
        if !values.is_empty() {
            gpu.queue.write_buffer(&self.params, 0, bytemuck::cast_slice(values));
        }
    }

    /// Updates the uniform and rebuilds the bind group when anything it references changed.
    pub fn update(&mut self, gpu: &Gpu, maps: &MeshMaps, bbox_min: glam::Vec3, bbox_size: glam::Vec3) {
        let mut baked = 0u32;
        for m in MeshMap::ALL {
            if maps.status(m) != MapStatus::Missing {
                baked |= 1 << m.index();
            }
        }
        let u = SceneUniform {
            bbox_min: bbox_min.extend(0.0).into(),
            bbox_size: bbox_size.max(glam::Vec3::splat(1e-6)).extend(0.0).into(),
            info: [baked, maps.layers, maps.res, 0],
        };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        // Mesh map textures are only replaced on reallocation, but rebinding on every
        // version change is cheap and keeps this simple.
        if !self.dirty && self.bind_group.is_some() && self.bound_maps_version == maps.version {
            return;
        }
        let tv = |v| wgpu::BindingResource::TextureView(v);
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler_clamp) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler_repeat) },
            wgpu::BindGroupEntry { binding: 2, resource: tv(&maps.position.view) },
            wgpu::BindGroupEntry { binding: 3, resource: tv(&maps.normal.view) },
            wgpu::BindGroupEntry { binding: 4, resource: tv(&maps.id.view) },
            wgpu::BindGroupEntry { binding: 5, resource: tv(&maps.ao.view) },
            wgpu::BindGroupEntry { binding: 6, resource: tv(&maps.curvature.view) },
            wgpu::BindGroupEntry { binding: 7, resource: tv(&maps.thickness.view) },
            wgpu::BindGroupEntry { binding: 8, resource: self.uniform.as_entire_binding() },
        ];
        for (i, p) in self.paint.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: 9 + i as u32, resource: tv(&p.view) });
        }
        entries.push(wgpu::BindGroupEntry { binding: 14, resource: tv(&self.mask.view) });
        entries.push(wgpu::BindGroupEntry { binding: 15, resource: tv(&self.images.view) });
        entries.push(wgpu::BindGroupEntry { binding: 16, resource: tv(&self.cache.view) });
        entries.push(wgpu::BindGroupEntry { binding: 17, resource: self.params.as_entire_binding() });
        self.bind_group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &self.layout,
            entries: &entries,
        }));
        self.dirty = false;
        self.bound_maps_version = maps.version;
    }
}
