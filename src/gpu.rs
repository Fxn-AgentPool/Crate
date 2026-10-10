//! GPU context shared by the windowed app and headless mode, plus small helpers.

use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Checks that the adapter can run Crate. Painting and baking read and write half-float
/// textures in place, which needs read-write storage support for Rgba16Float.
pub fn check_adapter(adapter: &wgpu::Adapter) -> Result<(), String> {
    let info = adapter.get_info();
    let mut missing = Vec::new();
    if !adapter.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        || !adapter
            .get_texture_format_features(wgpu::TextureFormat::Rgba16Float)
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::STORAGE_READ_WRITE)
    {
        missing.push("read-write storage textures (Rgba16Float)");
    }
    if !adapter.features().contains(wgpu::Features::FLOAT32_FILTERABLE) {
        missing.push("filterable 32-bit float textures");
    }
    if !adapter.features().contains(wgpu::Features::CLEAR_TEXTURE) {
        missing.push("texture clearing");
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "The GPU \"{}\" ({:?}, driver {}) does not support: {}. Crate needs a Vulkan 1.2 or Direct3D 12 GPU with current drivers.",
            info.name,
            info.backend,
            info.driver_info,
            missing.join(", ")
        ))
    }
}

/// Features Crate uses when the adapter has them.
pub fn wanted_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features()
        & (wgpu::Features::FLOAT32_FILTERABLE
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::CLEAR_TEXTURE)
}

/// Device descriptor requesting the adapter's own limits: the defaults are too small for
/// texture arrays holding up to 100 UDIM tiles and for the number of bindings the
/// compiled material shader uses.
pub fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        label: Some("crate device"),
        required_features: wanted_features(adapter),
        required_limits: adapter.limits(),
        ..Default::default()
    }
}

impl Gpu {
    /// Creates a device without a window, for headless rendering and tests.
    pub fn headless() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .context("No suitable GPU adapter (Vulkan 1.2 or Direct3D 12 required).")?;
        log::info!("Adapter: {:?}", adapter.get_info());
        check_adapter(&adapter).map_err(|e| anyhow::anyhow!(e))?;
        if std::env::var("PAINTER_PROBE").is_ok() {
            for f in [wgpu::TextureFormat::Rgba16Float, wgpu::TextureFormat::R32Float, wgpu::TextureFormat::Rgba8Unorm] {
                println!("{f:?}: {:?}", adapter.get_texture_format_features(f).flags);
            }
            println!("features: {:?}", adapter.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&device_descriptor(&adapter)))
            .context("Could not create the GPU device.")?;
        Ok(Self { device, queue })
    }

    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Reads back a 2D texture layer. Returns tightly packed rows.
    pub fn read_texture(
        &self,
        texture: &wgpu::Texture,
        layer: u32,
        bytes_per_pixel: u32,
    ) -> Vec<u8> {
        let (w, h) = (texture.width(), texture.height());
        let unpadded = w * bytes_per_pixel;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.wait_idle();
        let data = slice.get_mapped_range().expect("mapped readback buffer");
        let mut out = Vec::with_capacity((unpadded * h) as usize);
        for row in 0..h {
            let start = (row * padded) as usize;
            out.extend_from_slice(&data[start..start + unpadded as usize]);
        }
        drop(data);
        buffer.unmap();
        out
    }
}

pub fn shader(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

pub fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub fn texture_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    dim: wgpu::TextureViewDimension,
    filterable: bool,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: dim,
            multisampled: false,
        },
        count: None,
    }
}

pub fn sampler_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

pub fn render_target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    extra: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | extra,
        view_formats: &[],
    })
}
