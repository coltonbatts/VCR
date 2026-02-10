use std::num::NonZeroU32;
use std::sync::mpsc;

use anyhow::{anyhow, Context, Result};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use tiny_skia::{
    BlendMode, Color, FilterQuality, GradientStop, LinearGradient, Paint, Pixmap, PixmapPaint,
    Point, Rect, SpreadMode, Transform,
};
use wgpu::util::DeviceExt;

use crate::schema::{
    AssetLayer, ColorRgba, Environment, GradientDirection, Layer, LayerCommon, ProceduralLayer,
    ProceduralSource, PropertyValue, ScalarProperty, Vec2,
};

const BLEND_SHADER: &str = r#"
struct LayerUniform {
  opacity: f32,
  _pad0: f32,
  _pad1: f32,
  _pad2: f32,
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
const NO_GPU_ADAPTER_ERR: &str = "no suitable GPU adapter found (hardware or fallback)";

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
    _pad: [f32; 3],
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
    position_x: Option<ScalarProperty>,
    position_y: Option<ScalarProperty>,
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

struct GpuRenderer {
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

pub struct Renderer {
    backend: RendererBackend,
}

enum RendererBackend {
    Gpu(GpuRenderer),
    Software(SoftwareRenderer),
}

struct SoftwareRenderer {
    width: u32,
    height: u32,
    layers: Vec<SoftwareLayer>,
}

struct SoftwareLayer {
    z_index: i32,
    width: u32,
    height: u32,
    position: PropertyValue<Vec2>,
    position_x: Option<ScalarProperty>,
    position_y: Option<ScalarProperty>,
    scale: PropertyValue<Vec2>,
    rotation_degrees: ScalarProperty,
    opacity: ScalarProperty,
    source: SoftwareLayerSource,
}

enum SoftwareLayerSource {
    Asset { pixmap: Pixmap },
    Procedural(ProceduralSource),
}

impl GpuRenderer {
    pub async fn new(environment: &Environment, layers: &[Layer]) -> Result<Self> {
        let width = environment.resolution.width;
        let height = environment.resolution.height;

        let instance = wgpu::Instance::default();
        let adapter = if let Some(adapter) = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
        {
            adapter
        } else {
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: true,
                    compatible_surface: None,
                })
                .await
                .ok_or_else(|| anyhow!(NO_GPU_ADAPTER_ERR))?
        };

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("vcr-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .context("failed to request wgpu device")?;

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vcr-render-target"),
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
            label: Some("vcr-readback-buffer"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let blend_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vcr-layer-bind-group-layout"),
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
                label: Some("vcr-procedural-bind-group-layout"),
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
            label: Some("vcr-blend-shader"),
            source: wgpu::ShaderSource::Wgsl(BLEND_SHADER.into()),
        });
        let procedural_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vcr-procedural-shader"),
            source: wgpu::ShaderSource::Wgsl(PROCEDURAL_SHADER.into()),
        });

        let blend_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("vcr-blend-pipeline-layout"),
                bind_group_layouts: &[&blend_bind_group_layout],
                push_constant_ranges: &[],
            });

        let blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vcr-layer-pipeline"),
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
                label: Some("vcr-procedural-pipeline-layout"),
                bind_group_layouts: &[&procedural_bind_group_layout],
                push_constant_ranges: &[],
            });

        let procedural_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vcr-procedural-pipeline"),
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
            label: Some("vcr-layer-sampler"),
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
                label: Some("vcr-render-encoder"),
            });

        self.prepare_procedural_layers(frame_index, &mut encoder)?;

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vcr-render-pass"),
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
        let frame = copy_tight_rows(
            &mapped,
            self.unpadded_bytes_per_row,
            self.padded_bytes_per_row,
            self.height,
        )?;

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
                    label: Some("vcr-procedural-pass"),
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

impl Renderer {
    pub async fn new(environment: &Environment, layers: &[Layer]) -> Result<Self> {
        match GpuRenderer::new(environment, layers).await {
            Ok(gpu) => Ok(Self {
                backend: RendererBackend::Gpu(gpu),
            }),
            Err(error) if error.to_string().contains(NO_GPU_ADAPTER_ERR) => {
                let software = SoftwareRenderer::new(environment, layers)
                    .context("failed to initialize software renderer fallback")?;
                Ok(Self {
                    backend: RendererBackend::Software(software),
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn using_software(&self) -> bool {
        matches!(self.backend, RendererBackend::Software(_))
    }

    pub fn render_frame_rgba(&mut self, frame_index: u32) -> Result<Vec<u8>> {
        match &mut self.backend {
            RendererBackend::Gpu(renderer) => renderer.render_frame_rgba(frame_index),
            RendererBackend::Software(renderer) => renderer.render_frame_rgba(frame_index),
        }
    }
}

impl SoftwareRenderer {
    fn new(environment: &Environment, layers: &[Layer]) -> Result<Self> {
        let width = environment.resolution.width;
        let height = environment.resolution.height;
        let mut software_layers = Vec::with_capacity(layers.len());

        for layer in layers {
            let common = layer.common();
            let source = match layer {
                Layer::Asset(asset_layer) => SoftwareLayerSource::Asset {
                    pixmap: load_asset_pixmap(asset_layer)?,
                },
                Layer::Procedural(procedural_layer) => {
                    SoftwareLayerSource::Procedural(procedural_layer.procedural.clone())
                }
            };

            let (layer_width, layer_height) = match &source {
                SoftwareLayerSource::Asset { pixmap } => (pixmap.width(), pixmap.height()),
                SoftwareLayerSource::Procedural(_) => (width, height),
            };

            software_layers.push(SoftwareLayer {
                z_index: common.z_index,
                width: layer_width,
                height: layer_height,
                position: common.position.clone(),
                position_x: common.pos_x.clone(),
                position_y: common.pos_y.clone(),
                scale: common.scale.clone(),
                rotation_degrees: common.rotation_degrees.clone(),
                opacity: common.opacity.clone(),
                source,
            });
        }
        software_layers.sort_by_key(|layer| layer.z_index);

        Ok(Self {
            width,
            height,
            layers: software_layers,
        })
    }

    fn render_frame_rgba(&mut self, frame_index: u32) -> Result<Vec<u8>> {
        let mut output = Pixmap::new(self.width, self.height)
            .ok_or_else(|| anyhow!("failed to allocate software output pixmap"))?;
        output.fill(Color::from_rgba8(0, 0, 0, 0));

        for layer in &self.layers {
            self.render_layer(&mut output, layer, frame_index)?;
        }

        let mut frame = output.data().to_vec();
        unpremultiply_rgba_in_place(&mut frame);
        Ok(frame)
    }

    fn render_layer(
        &self,
        output: &mut Pixmap,
        layer: &SoftwareLayer,
        frame_index: u32,
    ) -> Result<()> {
        let opacity = layer.opacity.evaluate(frame_index)?.clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return Ok(());
        }

        let position = sample_position(
            &layer.position,
            layer.position_x.as_ref(),
            layer.position_y.as_ref(),
            frame_index,
        )?;
        let scale = layer.scale.sample(frame_index);
        let rotation = layer.rotation_degrees.evaluate(frame_index)?;
        let transform = layer_transform(
            position,
            scale,
            rotation,
            layer.width as f32,
            layer.height as f32,
        );

        match &layer.source {
            SoftwareLayerSource::Asset { pixmap } => {
                draw_layer_pixmap(output, pixmap.as_ref(), opacity, transform);
            }
            SoftwareLayerSource::Procedural(source) => {
                let procedural = render_procedural_pixmap(source, self.width, self.height)?;
                draw_layer_pixmap(output, procedural.as_ref(), opacity, transform);
            }
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
        label: Some(&format!("vcr-layer-{}", layer.common.id)),
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
        position_x: layer.common.pos_x.clone(),
        position_y: layer.common.pos_y.clone(),
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
        label: Some(&format!("vcr-procedural-layer-{}", layer.common.id)),
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
        label: Some(&format!("vcr-procedural-uniform-{}", layer.common.id)),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("vcr-procedural-bind-group-{}", layer.common.id)),
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
        position_x: layer.common.pos_x.clone(),
        position_y: layer.common.pos_y.clone(),
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
        _pad: [0.0; 3],
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("vcr-layer-uniform-{}", common.id)),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let blend_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("vcr-layer-bind-group-{}", common.id)),
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
        label: Some(&format!("vcr-layer-vertex-buffer-{}", common.id)),
        size: std::mem::size_of::<[Vertex; 6]>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let initial_vertices = build_layer_quad(
        frame_width,
        frame_height,
        layer_width,
        layer_height,
        common.sample_position(0)?,
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
            _pad: [0.0; 3],
        };
        queue.write_buffer(&layer.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
        layer.last_opacity = Some(opacity);
    }

    let position = sample_position(
        &layer.position,
        layer.position_x.as_ref(),
        layer.position_y.as_ref(),
        frame_index,
    )?;
    let vertices = build_layer_quad(
        frame_width,
        frame_height,
        layer.width,
        layer.height,
        position,
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

fn sample_position(
    position: &PropertyValue<Vec2>,
    position_x: Option<&ScalarProperty>,
    position_y: Option<&ScalarProperty>,
    frame_index: u32,
) -> Result<Vec2> {
    let mut sampled = position.sample(frame_index);
    if let Some(x) = position_x {
        sampled.x = x.evaluate(frame_index)?;
    }
    if let Some(y) = position_y {
        sampled.y = y.evaluate(frame_index)?;
    }
    Ok(sampled)
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

fn copy_tight_rows(
    mapped: &[u8],
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let required_len = padded_bytes_per_row as usize * height as usize;
    if mapped.len() < required_len {
        return Err(anyhow!(
            "mapped frame too small: expected at least {} bytes, got {}",
            required_len,
            mapped.len()
        ));
    }

    let mut frame = vec![0_u8; (unpadded_bytes_per_row * height) as usize];
    for row_index in 0..height as usize {
        let src_start = row_index * padded_bytes_per_row as usize;
        let src_end = src_start + unpadded_bytes_per_row as usize;
        let dst_start = row_index * unpadded_bytes_per_row as usize;
        let dst_end = dst_start + unpadded_bytes_per_row as usize;
        frame[dst_start..dst_end].copy_from_slice(&mapped[src_start..src_end]);
    }

    Ok(frame)
}

fn load_asset_pixmap(layer: &AssetLayer) -> Result<Pixmap> {
    let image = ImageReader::open(&layer.source_path)
        .with_context(|| format!("failed opening {}", layer.source_path.display()))?
        .decode()
        .with_context(|| format!("failed decoding {}", layer.source_path.display()))?
        .to_rgba8();
    let (width, height) = image.dimensions();

    let mut rgba = image.into_raw();
    premultiply_rgba_in_place(&mut rgba);

    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| {
        anyhow!(
            "failed to allocate software pixmap for '{}'",
            layer.common.id
        )
    })?;
    pixmap.data_mut().copy_from_slice(&rgba);
    Ok(pixmap)
}

fn draw_layer_pixmap(
    output: &mut Pixmap,
    source: tiny_skia::PixmapRef<'_>,
    opacity: f32,
    transform: Transform,
) {
    let mut paint = PixmapPaint::default();
    paint.opacity = opacity;
    paint.quality = FilterQuality::Bilinear;
    paint.blend_mode = BlendMode::SourceOver;
    output.draw_pixmap(0, 0, source, &paint, transform, None);
}

fn layer_transform(
    position: Vec2,
    scale: Vec2,
    rotation_degrees: f32,
    width: f32,
    height: f32,
) -> Transform {
    let scale_x = scale.x.max(0.0);
    let scale_y = scale.y.max(0.0);
    let radians = rotation_degrees.to_radians();
    let cos_theta = radians.cos();
    let sin_theta = radians.sin();

    let a = cos_theta * scale_x;
    let b = sin_theta * scale_x;
    let c = -sin_theta * scale_y;
    let d = cos_theta * scale_y;

    let half_w = width * 0.5;
    let half_h = height * 0.5;
    let center_world_x = position.x + (width * scale_x * 0.5);
    let center_world_y = position.y + (height * scale_y * 0.5);

    let tx = center_world_x - (a * half_w + c * half_h);
    let ty = center_world_y - (b * half_w + d * half_h);

    Transform::from_row(a, b, c, d, tx, ty)
}

fn render_procedural_pixmap(source: &ProceduralSource, width: u32, height: u32) -> Result<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| anyhow!("failed to allocate procedural software pixmap"))?;

    match source {
        ProceduralSource::SolidColor { color } => {
            pixmap.fill(color_to_skia(*color));
        }
        ProceduralSource::Gradient {
            start_color,
            end_color,
            direction,
        } => {
            let (start, end) = match direction {
                GradientDirection::Horizontal => {
                    (Point::from_xy(0.0, 0.0), Point::from_xy(width as f32, 0.0))
                }
                GradientDirection::Vertical => {
                    (Point::from_xy(0.0, 0.0), Point::from_xy(0.0, height as f32))
                }
            };
            let shader = LinearGradient::new(
                start,
                end,
                vec![
                    GradientStop::new(0.0, color_to_skia(*start_color)),
                    GradientStop::new(1.0, color_to_skia(*end_color)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
            .ok_or_else(|| anyhow!("failed to create procedural gradient shader"))?;

            let mut paint = Paint::default();
            paint.shader = shader;
            let rect = Rect::from_xywh(0.0, 0.0, width as f32, height as f32)
                .ok_or_else(|| anyhow!("invalid procedural gradient bounds"))?;
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    Ok(pixmap)
}

fn color_to_skia(color: ColorRgba) -> Color {
    Color::from_rgba8(
        f32_to_channel(color.r),
        f32_to_channel(color.g),
        f32_to_channel(color.b),
        f32_to_channel(color.a),
    )
}

fn f32_to_channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn premultiply_rgba_in_place(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        let alpha = pixel[3] as u16;
        pixel[0] = ((pixel[0] as u16 * alpha + 127) / 255) as u8;
        pixel[1] = ((pixel[1] as u16 * alpha + 127) / 255) as u8;
        pixel[2] = ((pixel[2] as u16 * alpha + 127) / 255) as u8;
    }
}

fn unpremultiply_rgba_in_place(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        let alpha = pixel[3];
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            continue;
        }

        let alpha_u16 = alpha as u16;
        pixel[0] = ((pixel[0] as u16 * 255 + (alpha_u16 / 2)) / alpha_u16).min(255) as u8;
        pixel[1] = ((pixel[1] as u16 * 255 + (alpha_u16 / 2)) / alpha_u16).min(255) as u8;
        pixel[2] = ((pixel[2] as u16 * 255 + (alpha_u16 / 2)) / alpha_u16).min(255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::copy_tight_rows;

    #[test]
    fn copy_tight_rows_strips_padding() {
        let mapped = vec![
            1, 2, 3, 4, 99, 99, 99, 99, // row 1: 4 bytes + 4 bytes pad
            5, 6, 7, 8, 88, 88, 88, 88, // row 2: 4 bytes + 4 bytes pad
        ];

        let tight = copy_tight_rows(&mapped, 4, 8, 2).expect("expected tight copy");
        assert_eq!(tight, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn copy_tight_rows_handles_already_tight_rows() {
        let mapped = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let tight = copy_tight_rows(&mapped, 4, 4, 2).expect("expected tight copy");
        assert_eq!(tight, mapped);
    }
}
