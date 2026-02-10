use std::num::NonZeroU32;
use std::sync::mpsc;

use anyhow::{anyhow, Context, Result};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use wgpu::util::DeviceExt;

use crate::schema::{
    AssetLayer, Environment, GradientDirection, Layer, LayerCommon, ProceduralLayer,
    ProceduralSource, PropertyValue, ScalarProperty, Vec2,
};

const BLEND_SHADER: &str = r#"
struct LayerUniform {
  opacity: f32,
  _pad0: vec3<f32>,
}

@group(0) @binding(0) var layer_tex: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;
@group(0) @binding(2) var<uniform> layer: LayerUniform;

struct VertexInput {
  @location(0) position: vec2<f32>,
  @location(1) uv: vec2<f32>,
}

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
  var out: VertexOutput;
  out.position = vec4<f32>(input.position, 0.0, 1.0);
  out.uv = input.uv;
  return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let tex = textureSample(layer_tex, layer_sampler, input.uv);
  return vec4<f32>(tex.rgb, tex.a * layer.opacity);
}
"#;

const PROCEDURAL_SHADER: &str = r#"
struct ProceduralUniform {
  kind: u32,
  axis: u32,
  _pad0: vec2<u32>,
  color_a: vec4<f32>,
  color_b: vec4<f32>,
}

@group(0) @binding(0) var<uniform> procedural: ProceduralUniform;

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
  var positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -3.0),
    vec2<f32>(-1.0, 1.0),
    vec2<f32>(3.0, 1.0)
  );

  var out: VertexOutput;
  let p = positions[vertex_index];
  out.position = vec4<f32>(p, 0.0, 1.0);
  out.uv = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
  return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let uv = clamp(input.uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
  if procedural.kind == 0u {
    return procedural.color_a;
  }

  let amount = select(uv.y, uv.x, procedural.axis == 0u);
  return mix(procedural.color_a, procedural.color_b, amount);
}
"#;

const EPSILON: f32 = 0.0001;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct LayerUniform {
    opacity: f32,
    _padding: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
struct ProceduralUniform {
    kind: u32,
    axis: u32,
    _padding: [u32; 2],
    color_a: [f32; 4],
    color_b: [f32; 4],
}

struct GpuLayer {
    z_index: i32,
    width: u32,
    height: u32,
    position: PropertyValue<Vec2>,
    scale: PropertyValue<Vec2>,
    rotation_degrees: ScalarProperty,
    opacity: ScalarProperty,
    all_properties_static: bool,
    uniform_buffer: wgpu::Buffer,
    blend_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    last_vertices: Option<[Vertex; 6]>,
    last_opacity: Option<f32>,
    source: GpuLayerSource,
}

impl GpuLayer {
    fn source_is_cached(&self) -> bool {
        match &self.source {
            GpuLayerSource::Asset { .. } => true,
            GpuLayerSource::Procedural(procedural) => {
                procedural.is_static && procedural.has_rendered
            }
        }
    }
}

enum GpuLayerSource {
    Asset { _texture: wgpu::Texture },
    Procedural(ProceduralGpu),
}

struct ProceduralGpu {
    source: ProceduralSource,
    is_static: bool,
    has_rendered: bool,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    last_uniform: Option<ProceduralUniform>,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    readback_buffer: wgpu::Buffer,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    blend_pipeline: wgpu::RenderPipeline,
    procedural_pipeline: wgpu::RenderPipeline,
    layers: Vec<GpuLayer>,
}

impl Renderer {
    pub async fn new(environment: &Environment, layers: &[Layer]) -> Result<Self> {
        let width = environment.resolution.width;
        let height = environment.resolution.height;

        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .ok_or_else(|| anyhow!("no suitable GPU adapter found"))?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ftc-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .context("failed to request wgpu device")?;

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ftc-render-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_bytes_per_row = width
            .checked_mul(4)
            .ok_or_else(|| anyhow!("frame width overflow when computing row bytes"))?;
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback_size = u64::from(padded_bytes_per_row) * u64::from(height);
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ftc-readback-buffer"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let blend_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ftc-layer-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(
                                std::mem::size_of::<LayerUniform>() as u64,
                            ),
                        },
                        count: None,
                    },
                ],
            });

        let procedural_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ftc-procedural-bind-group-layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<ProceduralUniform>() as u64,
                        ),
                    },
                    count: None,
                }],
            });

        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ftc-blend-shader"),
            source: wgpu::ShaderSource::Wgsl(BLEND_SHADER.into()),
        });
        let procedural_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ftc-procedural-shader"),
            source: wgpu::ShaderSource::Wgsl(PROCEDURAL_SHADER.into()),
        });

        let blend_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ftc-blend-pipeline-layout"),
                bind_group_layouts: &[&blend_bind_group_layout],
                push_constant_ranges: &[],
            });

        let blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ftc-layer-pipeline"),
            layout: Some(&blend_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blend_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blend_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
        });

        let procedural_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ftc-procedural-pipeline-layout"),
                bind_group_layouts: &[&procedural_bind_group_layout],
                push_constant_ranges: &[],
            });

        let procedural_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ftc-procedural-pipeline"),
            layout: Some(&procedural_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &procedural_shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &procedural_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ftc-layer-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let mut gpu_layers = Vec::with_capacity(layers.len());
        for layer in layers {
            let gpu_layer = match layer {
                Layer::Asset(asset_layer) => build_asset_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    asset_layer,
                    &blend_bind_group_layout,
                    &sampler,
                )?,
                Layer::Procedural(procedural_layer) => build_procedural_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    procedural_layer,
                    &blend_bind_group_layout,
                    &procedural_bind_group_layout,
                    &sampler,
                )?,
            };
            gpu_layers.push(gpu_layer);
        }
        gpu_layers.sort_by_key(|layer| layer.z_index);

        Ok(Self {
            device,
            queue,
            width,
            height,
            output_texture,
            output_view,
            readback_buffer,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            blend_pipeline,
            procedural_pipeline,
            layers: gpu_layers,
        })
    }

    pub fn render_frame(&mut self, frame_index: u32) -> Result<()> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ftc-render-encoder"),
            });

        self.prepare_procedural_layers(frame_index, &mut encoder)?;

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ftc-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            render_pass.set_pipeline(&self.blend_pipeline);
            for layer in &mut self.layers {
                let can_reuse_cached =
                    frame_index > 0 && layer.all_properties_static && layer.source_is_cached();
                if !can_reuse_cached {
                    refresh_layer_draw_state(
                        &self.queue,
                        self.width,
                        self.height,
                        layer,
                        frame_index,
                    )?;
                }

                render_pass.set_bind_group(0, &layer.blend_bind_group, &[]);
                render_pass.set_vertex_buffer(0, layer.vertex_buffer.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        let padded_bytes_per_row = NonZeroU32::new(self.padded_bytes_per_row)
            .ok_or_else(|| anyhow!("invalid padded row size {}", self.padded_bytes_per_row))?;
        let rows_per_image = NonZeroU32::new(self.height)
            .ok_or_else(|| anyhow!("invalid render height {}", self.height))?;

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.readback_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row.get()),
                    rows_per_image: Some(rows_per_image.get()),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    pub fn read_buffer(&mut self) -> Result<Vec<u8>> {
        let buffer_slice = self.readback_buffer.slice(..);
        let (sender, receiver) = mpsc::channel();

        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);

        receiver
            .recv()
            .map_err(|_| anyhow!("failed receiving GPU map callback"))?
            .context("GPU buffer mapping failed")?;

        let mapped = buffer_slice.get_mapped_range();
        let mut frame = vec![0_u8; (self.unpadded_bytes_per_row * self.height) as usize];

        for (row_index, chunk) in mapped
            .chunks(self.padded_bytes_per_row as usize)
            .take(self.height as usize)
            .enumerate()
        {
            let dst_start = row_index * self.unpadded_bytes_per_row as usize;
            let dst_end = dst_start + self.unpadded_bytes_per_row as usize;
            frame[dst_start..dst_end]
                .copy_from_slice(&chunk[..self.unpadded_bytes_per_row as usize]);
        }

        drop(mapped);
        self.readback_buffer.unmap();
        Ok(frame)
    }

    pub fn render_frame_rgba(&mut self, frame_index: u32) -> Result<Vec<u8>> {
        self.render_frame(frame_index)?;
        self.read_buffer()
    }

    fn prepare_procedural_layers(
        &mut self,
        _frame_index: u32,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<()> {
        for layer in &mut self.layers {
            let GpuLayerSource::Procedural(procedural) = &mut layer.source else {
                continue;
            };

            let needs_update = !procedural.has_rendered || !procedural.is_static;
            if !needs_update {
                continue;
            }

            let uniform = procedural_uniform(&procedural.source);
            if procedural.last_uniform != Some(uniform) {
                self.queue.write_buffer(
                    &procedural.uniform_buffer,
                    0,
                    bytemuck::bytes_of(&uniform),
                );
                procedural.last_uniform = Some(uniform);
            }

            {
                let mut procedural_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ftc-procedural-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &procedural.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                });

                procedural_pass.set_pipeline(&self.procedural_pipeline);
                procedural_pass.set_bind_group(0, &procedural.bind_group, &[]);
                procedural_pass.draw(0..3, 0..1);
            }

            procedural.has_rendered = true;
        }

        Ok(())
    }
}

fn build_asset_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    layer: &AssetLayer,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> Result<GpuLayer> {
    let image = ImageReader::open(&layer.source_path)
        .with_context(|| format!("failed opening {}", layer.source_path.display()))?
        .decode()
        .with_context(|| format!("failed decoding {}", layer.source_path.display()))?
        .to_rgba8();
    let (layer_width, layer_height) = image.dimensions();

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("ftc-layer-{}", layer.common.id)),
        size: wgpu::Extent3d {
            width: layer_width,
            height: layer_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let bytes_per_row = NonZeroU32::new(layer_width.saturating_mul(4)).ok_or_else(|| {
        anyhow!(
            "layer '{}' has invalid width {}",
            layer.common.id,
            layer_width
        )
    })?;
    let rows_per_image = NonZeroU32::new(layer_height).ok_or_else(|| {
        anyhow!(
            "layer '{}' has invalid height {}",
            layer.common.id,
            layer_height
        )
    })?;

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        image.as_raw(),
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row.get()),
            rows_per_image: Some(rows_per_image.get()),
        },
        wgpu::Extent3d {
            width: layer_width,
            height: layer_height,
            depth_or_array_layers: 1,
        },
    );

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        layer_width,
        layer_height,
        &texture_view,
        blend_bind_group_layout,
        sampler,
    )?;

    Ok(GpuLayer {
        z_index: layer.common.z_index,
        width: layer_width,
        height: layer_height,
        position: layer.common.position.clone(),
        scale: layer.common.scale.clone(),
        rotation_degrees: layer.common.rotation_degrees.clone(),
        opacity: layer.common.opacity.clone(),
        all_properties_static: layer.common.has_static_properties(),
        uniform_buffer: draw_resources.uniform_buffer,
        blend_bind_group: draw_resources.blend_bind_group,
        vertex_buffer: draw_resources.vertex_buffer,
        last_vertices: Some(draw_resources.initial_vertices),
        last_opacity: Some(draw_resources.initial_opacity),
        source: GpuLayerSource::Asset { _texture: texture },
    })
}

fn build_procedural_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    layer: &ProceduralLayer,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    procedural_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> Result<GpuLayer> {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("ftc-procedural-layer-{}", layer.common.id)),
        size: wgpu::Extent3d {
            width: frame_width,
            height: frame_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let uniform = procedural_uniform(&layer.procedural);
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("ftc-procedural-uniform-{}", layer.common.id)),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("ftc-procedural-bind-group-{}", layer.common.id)),
        layout: procedural_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        frame_width,
        frame_height,
        &view,
        blend_bind_group_layout,
        sampler,
    )?;

    let procedural_gpu = ProceduralGpu {
        source: layer.procedural.clone(),
        is_static: true,
        has_rendered: false,
        uniform_buffer,
        bind_group,
        _texture: texture,
        view,
        last_uniform: None,
    };

    Ok(GpuLayer {
        z_index: layer.common.z_index,
        width: frame_width,
        height: frame_height,
        position: layer.common.position.clone(),
        scale: layer.common.scale.clone(),
        rotation_degrees: layer.common.rotation_degrees.clone(),
        opacity: layer.common.opacity.clone(),
        all_properties_static: layer.common.has_static_properties(),
        uniform_buffer: draw_resources.uniform_buffer,
        blend_bind_group: draw_resources.blend_bind_group,
        vertex_buffer: draw_resources.vertex_buffer,
        last_vertices: Some(draw_resources.initial_vertices),
        last_opacity: Some(draw_resources.initial_opacity),
        source: GpuLayerSource::Procedural(procedural_gpu),
    })
}

struct LayerDrawResources {
    uniform_buffer: wgpu::Buffer,
    blend_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    initial_vertices: [Vertex; 6],
    initial_opacity: f32,
}

fn build_layer_draw_resources(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    common: &LayerCommon,
    layer_width: u32,
    layer_height: u32,
    sampled_texture_view: &wgpu::TextureView,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> Result<LayerDrawResources> {
    let opacity = common.opacity.evaluate(0)?.clamp(0.0, 1.0);
    let uniform = LayerUniform {
        opacity,
        _padding: [0.0; 3],
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("ftc-layer-uniform-{}", common.id)),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let blend_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("ftc-layer-bind-group-{}", common.id)),
        layout: blend_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(sampled_texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform_buffer.as_entire_binding(),
            },
        ],
    });

    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("ftc-layer-vertex-buffer-{}", common.id)),
        size: std::mem::size_of::<[Vertex; 6]>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let initial_vertices = build_layer_quad(
        frame_width,
        frame_height,
        layer_width,
        layer_height,
        common.position.sample(0),
        common.scale.sample(0),
        common.rotation_degrees.evaluate(0)?,
    );
    queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&initial_vertices));

    Ok(LayerDrawResources {
        uniform_buffer,
        blend_bind_group,
        vertex_buffer,
        initial_vertices,
        initial_opacity: opacity,
    })
}

fn refresh_layer_draw_state(
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    layer: &mut GpuLayer,
    frame_index: u32,
) -> Result<()> {
    let opacity = layer.opacity.evaluate(frame_index)?.clamp(0.0, 1.0);
    if layer
        .last_opacity
        .map_or(true, |previous| (previous - opacity).abs() > EPSILON)
    {
        let uniform = LayerUniform {
            opacity,
            _padding: [0.0; 3],
        };
        queue.write_buffer(&layer.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
        layer.last_opacity = Some(opacity);
    }

    let vertices = build_layer_quad(
        frame_width,
        frame_height,
        layer.width,
        layer.height,
        layer.position.sample(frame_index),
        layer.scale.sample(frame_index),
        layer.rotation_degrees.evaluate(frame_index)?,
    );

    if layer
        .last_vertices
        .as_ref()
        .map_or(true, |cached| !vertices_approx_eq(cached, &vertices))
    {
        queue.write_buffer(&layer.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        layer.last_vertices = Some(vertices);
    }

    Ok(())
}

fn procedural_uniform(source: &ProceduralSource) -> ProceduralUniform {
    match source {
        ProceduralSource::SolidColor { color } => ProceduralUniform {
            kind: 0,
            axis: 0,
            _padding: [0; 2],
            color_a: color.as_array(),
            color_b: color.as_array(),
        },
        ProceduralSource::Gradient {
            start_color,
            end_color,
            direction,
        } => {
            let axis = match direction {
                GradientDirection::Horizontal => 0,
                GradientDirection::Vertical => 1,
            };
            ProceduralUniform {
                kind: 1,
                axis,
                _padding: [0; 2],
                color_a: start_color.as_array(),
                color_b: end_color.as_array(),
            }
        }
    }
}

fn vertices_approx_eq(left: &[Vertex; 6], right: &[Vertex; 6]) -> bool {
    for (lhs, rhs) in left.iter().zip(right.iter()) {
        for (a, b) in lhs.position.iter().zip(rhs.position.iter()) {
            if (a - b).abs() > EPSILON {
                return false;
            }
        }
        for (a, b) in lhs.uv.iter().zip(rhs.uv.iter()) {
            if (a - b).abs() > EPSILON {
                return false;
            }
        }
    }
    true
}

fn build_layer_quad(
    frame_width: u32,
    frame_height: u32,
    layer_width: u32,
    layer_height: u32,
    position: Vec2,
    scale: Vec2,
    rotation_degrees: f32,
) -> [Vertex; 6] {
    let scaled_width = layer_width as f32 * scale.x.max(0.0);
    let scaled_height = layer_height as f32 * scale.y.max(0.0);

    let center_x = position.x + (scaled_width * 0.5);
    let center_y = position.y + (scaled_height * 0.5);
    let half_w = scaled_width * 0.5;
    let half_h = scaled_height * 0.5;

    let radians = rotation_degrees.to_radians();
    let sin_theta = radians.sin();
    let cos_theta = radians.cos();

    let top_left = rotate_point(-half_w, -half_h, cos_theta, sin_theta, center_x, center_y);
    let top_right = rotate_point(half_w, -half_h, cos_theta, sin_theta, center_x, center_y);
    let bottom_left = rotate_point(-half_w, half_h, cos_theta, sin_theta, center_x, center_y);
    let bottom_right = rotate_point(half_w, half_h, cos_theta, sin_theta, center_x, center_y);

    let tl = to_clip(top_left.0, top_left.1, frame_width, frame_height);
    let tr = to_clip(top_right.0, top_right.1, frame_width, frame_height);
    let bl = to_clip(bottom_left.0, bottom_left.1, frame_width, frame_height);
    let br = to_clip(bottom_right.0, bottom_right.1, frame_width, frame_height);

    [
        Vertex {
            position: tl,
            uv: [0.0, 0.0],
        },
        Vertex {
            position: bl,
            uv: [0.0, 1.0],
        },
        Vertex {
            position: tr,
            uv: [1.0, 0.0],
        },
        Vertex {
            position: tr,
            uv: [1.0, 0.0],
        },
        Vertex {
            position: bl,
            uv: [0.0, 1.0],
        },
        Vertex {
            position: br,
            uv: [1.0, 1.0],
        },
    ]
}

fn rotate_point(
    x: f32,
    y: f32,
    cos_theta: f32,
    sin_theta: f32,
    center_x: f32,
    center_y: f32,
) -> (f32, f32) {
    let rotated_x = x * cos_theta - y * sin_theta;
    let rotated_y = x * sin_theta + y * cos_theta;
    (center_x + rotated_x, center_y + rotated_y)
}

fn to_clip(x: f32, y: f32, width: u32, height: u32) -> [f32; 2] {
    let clip_x = (x / width as f32) * 2.0 - 1.0;
    let clip_y = 1.0 - (y / height as f32) * 2.0;
    [clip_x, clip_y]
}

fn align_to(value: u32, alignment: u32) -> u32 {
    let mask = alignment - 1;
    (value + mask) & !mask
}
