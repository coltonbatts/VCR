use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::mpsc;

use anyhow::{anyhow, bail, Context, Result};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, PathBuilder,
    Pixmap, PixmapPaint, Point, Rect, SpreadMode, Transform,
};
use wgpu::util::DeviceExt;

use crate::schema::{
    Anchor, AssetLayer, ColorRgba, Environment, ExpressionContext, GradientDirection, Group, Layer,
    ImageLayer, LayerCommon, Manifest, ModulatorBinding, ModulatorMap, Parameters,
    ProceduralLayer,
    ProceduralSource, PropertyValue, ScalarProperty, TextLayer, TimingControls, Vec2,
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
  p0: vec2<f32>,
  p1: vec2<f32>,
  p2: vec2<f32>,
  radius: f32,
  _pad1: f32,
  _pad2: vec4<f32>,
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

fn sign_func(p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>) -> f32 {
    return (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let uv = clamp(input.uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
  
  if procedural.kind == 0u {
    return procedural.color_a;
  }
  
  if procedural.kind == 1u {
    let amount = select(uv.y, uv.x, procedural.axis == 0u);
    return mix(procedural.color_a, procedural.color_b, amount);
  }

  if procedural.kind == 2u {
    let d1 = sign_func(uv, procedural.p0, procedural.p1);
    let d2 = sign_func(uv, procedural.p1, procedural.p2);
    let d3 = sign_func(uv, procedural.p2, procedural.p0);

    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);

    if !(has_neg && has_pos) {
       // Outside the triangle, return transparent
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }
  }

  if (procedural.kind == 3u) { // Circle
    let dist = distance(uv, procedural.p0);
    if (dist < procedural.radius) {
        return procedural.color_a;
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }

  // Default fallback
  return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
"#;

const EPSILON: f32 = 0.0001;
const NO_GPU_ADAPTER_ERR: &str = "no suitable GPU adapter found";

#[derive(Debug, Clone, Default)]
pub struct RenderSceneData {
    pub seed: u64,
    pub params: Parameters,
    pub modulators: ModulatorMap,
    pub groups: Vec<Group>,
}

impl RenderSceneData {
    pub fn from_manifest(manifest: &Manifest) -> Self {
        Self {
            seed: manifest.seed,
            params: manifest.params.clone(),
            modulators: manifest.modulators.clone(),
            groups: manifest.groups.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayerDebugState {
    pub id: String,
    pub name: Option<String>,
    pub stable_id: Option<String>,
    pub z_index: i32,
    pub visible: bool,
    pub position: Vec2,
    pub scale: Vec2,
    pub rotation_degrees: f32,
    pub opacity: f32,
}

pub fn evaluate_manifest_layers_at_frame(
    manifest: &Manifest,
    frame_index: u32,
) -> Result<Vec<LayerDebugState>> {
    let scene = RenderSceneData::from_manifest(manifest);
    let groups_by_id = scene
        .groups
        .iter()
        .cloned()
        .map(|group| (group.id.clone(), group))
        .collect::<BTreeMap<_, _>>();

    let mut states = Vec::with_capacity(manifest.layers.len());
    for layer in &manifest.layers {
        let common = layer.common();
        let group_chain = resolve_group_chain(common, &groups_by_id)?;
        let evaluated = evaluate_layer_state(
            common.id.as_str(),
            &common.position,
            common.pos_x.as_ref(),
            common.pos_y.as_ref(),
            &common.scale,
            &common.rotation_degrees,
            &common.opacity,
            common.timing_controls(),
            &common.modulators,
            &group_chain,
            frame_index,
            manifest.environment.fps,
            &scene.params,
            scene.seed,
            &scene.modulators,
        )?;

        let (visible, position, scale, rotation_degrees, opacity) = if let Some(state) = evaluated {
            (
                true,
                state.position,
                state.scale,
                state.rotation_degrees,
                state.opacity,
            )
        } else {
            (
                false,
                Vec2 { x: 0.0, y: 0.0 },
                Vec2 { x: 1.0, y: 1.0 },
                0.0,
                0.0,
            )
        };

        states.push(LayerDebugState {
            id: common.id.clone(),
            name: common.name.clone(),
            stable_id: common.stable_id.clone(),
            z_index: common.z_index,
            visible,
            position,
            scale,
            rotation_degrees,
            opacity,
        });
    }

    states.sort_by_key(|state| state.z_index);
    Ok(states)
}

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
    p0: [f32; 2], // center for Circle
    p1: [f32; 2],
    p2: [f32; 2],
    radius: f32,
    _pad_radius: [f32; 5],
}

struct GpuLayer {
    id: String,
    z_index: i32,
    width: u32,
    height: u32,
    position: PropertyValue<Vec2>,
    position_x: Option<ScalarProperty>,
    position_y: Option<ScalarProperty>,
    scale: PropertyValue<Vec2>,
    rotation_degrees: ScalarProperty,
    opacity: ScalarProperty,
    timing: TimingControls,
    modulators: Vec<ModulatorBinding>,
    group_chain: Vec<Group>,
    all_properties_static: bool,
    uniform_buffer: wgpu::Buffer,
    blend_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    last_vertices: Option<[Vertex; 6]>,
    last_opacity: Option<f32>,
    anchor: Anchor,
    source: GpuLayerSource,
}

impl GpuLayer {
    fn source_is_cached(&self) -> bool {
        match &self.source {
            GpuLayerSource::Asset { .. } => true,
            GpuLayerSource::Procedural(gpu) => gpu.has_rendered,
            GpuLayerSource::Text { .. } => true,
        }
    }
}

enum GpuLayerSource {
    Asset { _texture: wgpu::Texture },
    Procedural(ProceduralGpu),
    Text { _texture: wgpu::Texture },
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
    adapter_name: String,
    adapter_backend: wgpu::Backend,
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    fps: u32,
    seed: u64,
    params: Parameters,
    modulators: ModulatorMap,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    readback_buffer: wgpu::Buffer,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    blend_pipeline: wgpu::RenderPipeline,
    procedural_pipeline: wgpu::RenderPipeline,
    layers: Vec<GpuLayer>,
}

#[cfg(target_os = "macos")]
const PREFERRED_BACKENDS: wgpu::Backends = wgpu::Backends::METAL;

#[cfg(not(target_os = "macos"))]
const PREFERRED_BACKENDS: wgpu::Backends = wgpu::Backends::PRIMARY;

async fn request_best_adapter(instance: &wgpu::Instance) -> Result<wgpu::Adapter> {
    if let Some(adapter) = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
    {
        return Ok(adapter);
    }

    if let Some(adapter) = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
    {
        return Ok(adapter);
    }

    if let Some(adapter) = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: true,
            compatible_surface: None,
        })
        .await
    {
        return Ok(adapter);
    }

    let enumerated = instance
        .enumerate_adapters(wgpu::Backends::all())
        .into_iter()
        .map(|adapter| {
            let info = adapter.get_info();
            format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
        })
        .collect::<Vec<_>>();

    if enumerated.is_empty() {
        bail!("{NO_GPU_ADAPTER_ERR}; enumerate_adapters() returned none");
    }

    bail!(
        "{NO_GPU_ADAPTER_ERR}; enumerate_adapters() saw: {}",
        enumerated.join(", ")
    );
}

pub struct Renderer {
    backend: RendererBackend,
    backend_reason: String,
}

enum RendererBackend {
    Gpu(GpuRenderer),
    Software(SoftwareRenderer),
}

struct SoftwareRenderer {
    width: u32,
    height: u32,
    fps: u32,
    seed: u64,
    params: Parameters,
    modulators: ModulatorMap,
    layers: Vec<SoftwareLayer>,
}

struct SoftwareLayer {
    id: String,
    z_index: i32,
    width: u32,
    height: u32,
    position: PropertyValue<Vec2>,
    position_x: Option<ScalarProperty>,
    position_y: Option<ScalarProperty>,
    scale: PropertyValue<Vec2>,
    rotation_degrees: ScalarProperty,
    opacity: ScalarProperty,
    timing: TimingControls,
    modulators: Vec<ModulatorBinding>,
    group_chain: Vec<Group>,
    anchor: Anchor,
    source: SoftwareLayerSource,
}

enum SoftwareLayerSource {
    Asset { pixmap: Pixmap },
    Procedural(ProceduralSource),
    Text { pixmap: Pixmap },
}

impl GpuRenderer {
    pub async fn new(
        environment: &Environment,
        layers: &[Layer],
        scene: &RenderSceneData,
    ) -> Result<Self> {
        let width = environment.resolution.width;
        let height = environment.resolution.height;
        let groups_by_id = scene
            .groups
            .iter()
            .cloned()
            .map(|group| (group.id.clone(), group))
            .collect::<BTreeMap<_, _>>();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: PREFERRED_BACKENDS,
            ..Default::default()
        });
        let adapter = request_best_adapter(&instance).await?;
        let adapter_info = adapter.get_info();

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
            let common = layer.common();
            let group_chain = resolve_group_chain(common, &groups_by_id)?;
            let gpu_layer = match layer {
                Layer::Asset(asset_layer) => build_asset_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    asset_layer,
                    group_chain.clone(),
                    &blend_bind_group_layout,
                    &sampler,
                    &groups_by_id,
                    &scene.params,
                    &scene.modulators,
                    scene.seed,
                    environment.fps,
                )?,
                Layer::Image(image_layer) => build_image_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    image_layer,
                    group_chain.clone(),
                    &blend_bind_group_layout,
                    &sampler,
                    &groups_by_id,
                    &scene.params,
                    &scene.modulators,
                    scene.seed,
                    environment.fps,
                )?,
                Layer::Procedural(procedural_layer) => build_procedural_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    procedural_layer,
                    group_chain,
                    &blend_bind_group_layout,
                    &procedural_bind_group_layout,
                    &sampler,
                    &groups_by_id,
                    &scene.params,
                    &scene.modulators,
                    scene.seed,
                    environment.fps,
                )?,
                Layer::Text(text_layer) => build_text_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    text_layer,
                    group_chain,
                    &blend_bind_group_layout,
                    &sampler,
                    &groups_by_id,
                    &scene.params,
                    &scene.modulators,
                    scene.seed,
                    environment.fps,
                )?,
            };
            gpu_layers.push(gpu_layer);
        }
        gpu_layers.sort_by_key(|layer| layer.z_index);

        Ok(Self {
            adapter_name: adapter_info.name,
            adapter_backend: adapter_info.backend,
            device,
            queue,
            width,
            height,
            fps: environment.fps,
            seed: scene.seed,
            params: scene.params.clone(),
            modulators: scene.modulators.clone(),
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
                        self.fps,
                        self.seed,
                        &self.params,
                        &self.modulators,
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
    pub async fn new_with_scene(
        environment: &Environment,
        layers: &[Layer],
        scene: RenderSceneData,
    ) -> Result<Self> {
        match GpuRenderer::new(environment, layers, &scene).await {
            Ok(gpu) => Ok(Self {
                backend_reason: format!(
                    "adapter '{}' ({:?})",
                    gpu.adapter_name, gpu.adapter_backend
                ),
                backend: RendererBackend::Gpu(gpu),
            }),
            Err(error) if error.to_string().contains(NO_GPU_ADAPTER_ERR) => {
                let software = SoftwareRenderer::new(environment, layers, &scene)
                    .context("failed to initialize software renderer fallback")?;
                Ok(Self {
                    backend: RendererBackend::Software(software),
                    backend_reason: error.to_string(),
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            RendererBackend::Gpu(_) => "GPU",
            RendererBackend::Software(_) => "CPU",
        }
    }

    pub fn backend_reason(&self) -> &str {
        &self.backend_reason
    }

    pub fn render_frame_rgba(&mut self, frame_index: u32) -> Result<Vec<u8>> {
        match &mut self.backend {
            RendererBackend::Gpu(renderer) => renderer.render_frame_rgba(frame_index),
            RendererBackend::Software(renderer) => renderer.render_frame_rgba(frame_index),
        }
    }
}

impl SoftwareRenderer {
    fn new(environment: &Environment, layers: &[Layer], scene: &RenderSceneData) -> Result<Self> {
        let width = environment.resolution.width;
        let height = environment.resolution.height;
        let groups_by_id = scene
            .groups
            .iter()
            .cloned()
            .map(|group| (group.id.clone(), group))
            .collect::<BTreeMap<_, _>>();
        let mut software_layers = Vec::with_capacity(layers.len());

        for layer in layers {
            let common = layer.common();
            let group_chain = resolve_group_chain(common, &groups_by_id)?;
            let source = match layer {
                Layer::Asset(asset_layer) => SoftwareLayerSource::Asset {
                    pixmap: load_asset_pixmap(asset_layer)?,
                },
                Layer::Image(image_layer) => SoftwareLayerSource::Asset {
                    pixmap: load_image_pixmap(image_layer)?,
                },
                Layer::Procedural(procedural_layer) => {
                    SoftwareLayerSource::Procedural(procedural_layer.procedural.clone())
                }
                Layer::Text(text_layer) => SoftwareLayerSource::Text {
                    pixmap: render_text_to_pixmap(text_layer)?,
                },
            };

            let (layer_width, layer_height) = match &source {
                SoftwareLayerSource::Asset { pixmap } => (pixmap.width(), pixmap.height()),
                SoftwareLayerSource::Procedural(_) => (width, height),
                SoftwareLayerSource::Text { pixmap } => (pixmap.width(), pixmap.height()),
            };

            software_layers.push(SoftwareLayer {
                id: common.id.clone(),
                z_index: common.z_index,
                width: layer_width,
                height: layer_height,
                position: common.position.clone(),
                position_x: common.pos_x.clone(),
                position_y: common.pos_y.clone(),
                scale: common.scale.clone(),
                rotation_degrees: common.rotation_degrees.clone(),
                opacity: common.opacity.clone(),
                timing: common.timing_controls(),
                modulators: common.modulators.clone(),
                group_chain,
                anchor: common.anchor,
                source,
            });
        }
        software_layers.sort_by_key(|layer| layer.z_index);

        Ok(Self {
            width,
            height,
            fps: environment.fps,
            seed: scene.seed,
            params: scene.params.clone(),
            modulators: scene.modulators.clone(),
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
        let Some(state) = evaluate_layer_state(
            &layer.id,
            &layer.position,
            layer.position_x.as_ref(),
            layer.position_y.as_ref(),
            &layer.scale,
            &layer.rotation_degrees,
            &layer.opacity,
            layer.timing,
            &layer.modulators,
            &layer.group_chain,
            frame_index,
            self.fps,
            &self.params,
            self.seed,
            &self.modulators,
        )?
        else {
            return Ok(());
        };
        let opacity = state.opacity.clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return Ok(());
        }
        let transform = layer_transform(
            state.position,
            state.scale,
            state.rotation_degrees,
            layer.width as f32,
            layer.height as f32,
            layer.anchor,
        );

        match &layer.source {
            SoftwareLayerSource::Asset { pixmap } => {
                draw_layer_pixmap(output, pixmap.as_ref(), opacity, transform);
            }
            SoftwareLayerSource::Procedural(source) => {
                let procedural = render_procedural_pixmap(source, self.width, self.height)?;
                draw_layer_pixmap(output, procedural.as_ref(), opacity, transform);
            }
            SoftwareLayerSource::Text { pixmap } => {
                draw_layer_pixmap(output, pixmap.as_ref(), opacity, transform);
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
    group_chain: Vec<Group>,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    groups_by_id: &BTreeMap<String, Group>,
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64,
    fps: u32,
) -> Result<GpuLayer> {
    build_bitmap_layer(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        &layer.source_path,
        group_chain,
        blend_bind_group_layout,
        sampler,
        groups_by_id,
        params,
        modulators,
        seed,
        fps,
    )
}

fn build_image_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    layer: &ImageLayer,
    group_chain: Vec<Group>,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    groups_by_id: &BTreeMap<String, Group>,
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64,
    fps: u32,
) -> Result<GpuLayer> {
    build_bitmap_layer(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        &layer.image.path,
        group_chain,
        blend_bind_group_layout,
        sampler,
        groups_by_id,
        params,
        modulators,
        seed,
        fps,
    )
}

fn build_bitmap_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    common: &LayerCommon,
    image_path: &Path,
    group_chain: Vec<Group>,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    groups_by_id: &BTreeMap<String, Group>,
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64, fps: u32,
) -> Result<GpuLayer> {
    let image = load_rgba_image(image_path, &common.id)?;
    let (layer_width, layer_height) = image.dimensions();

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("vcr-layer-{}", common.id)),
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

    let bytes_per_row = NonZeroU32::new(layer_width.saturating_mul(4))
        .ok_or_else(|| anyhow!("layer '{}' has invalid width {}", common.id, layer_width))?;
    let rows_per_image = NonZeroU32::new(layer_height)
        .ok_or_else(|| anyhow!("layer '{}' has invalid height {}", common.id, layer_height))?;

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

    let group_chain_ref = resolve_group_chain(common, groups_by_id)?;
    let state = evaluate_layer_state(
        &common.id,
        &common.position,
        common.pos_x.as_ref(),
        common.pos_y.as_ref(),
        &common.scale,
        &common.rotation_degrees,
        &common.opacity,
        common.timing_controls(),
        &common.modulators,
        &group_chain_ref,
        0,
        fps,
        params,
        seed,
        modulators,
    )?.ok_or_else(|| anyhow!("layer '{}' not active at frame 0", common.id))?;

    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        common,
        state.position,
        state.scale,
        state.rotation_degrees,
        layer_width,
        layer_height,
        &texture_view,
        blend_bind_group_layout,
        sampler,
    )?;

    Ok(GpuLayer {
        id: common.id.clone(),
        z_index: common.z_index,
        width: layer_width,
        height: layer_height,
        position: common.position.clone(),
        position_x: common.pos_x.clone(),
        position_y: common.pos_y.clone(),
        scale: common.scale.clone(),
        rotation_degrees: common.rotation_degrees.clone(),
        opacity: common.opacity.clone(),
        timing: common.timing_controls(),
        modulators: common.modulators.clone(),
        all_properties_static: common.has_static_properties()
            && group_chain.iter().all(Group::has_static_properties),
        group_chain, anchor: common.anchor,
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
    group_chain: Vec<Group>,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    procedural_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    groups_by_id: &BTreeMap<String, Group>,
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64,
    fps: u32,
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

    let state = evaluate_layer_state(
        &layer.common.id,
        &layer.common.position,
        layer.common.pos_x.as_ref(),
        layer.common.pos_y.as_ref(),
        &layer.common.scale,
        &layer.common.rotation_degrees,
        &layer.common.opacity,
        layer.common.timing_controls(),
        &layer.common.modulators,
        &group_chain,
        0,
        fps,
        params,
        seed,
        modulators,
    )?.ok_or_else(|| anyhow!("layer not active at frame 0"))?;

    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        state.position,
        state.scale,
        state.rotation_degrees,
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
        id: layer.common.id.clone(),
        z_index: layer.common.z_index,
        width: frame_width,
        height: frame_height,
        position: layer.common.position.clone(),
        position_x: layer.common.pos_x.clone(),
        position_y: layer.common.pos_y.clone(),
        scale: layer.common.scale.clone(),
        rotation_degrees: layer.common.rotation_degrees.clone(),
        opacity: layer.common.opacity.clone(),
        timing: layer.common.timing_controls(),
        modulators: layer.common.modulators.clone(),
        all_properties_static: layer.common.has_static_properties()
            && group_chain.iter().all(Group::has_static_properties),
        group_chain, anchor: layer.common.anchor,
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
    position: Vec2,
    scale: Vec2,
    rotation_degrees: f32,
    layer_width: u32,
    layer_height: u32,
    sampled_texture_view: &wgpu::TextureView,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> Result<LayerDrawResources> {
    let opacity = 1.0;
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
        position,
        scale,
        rotation_degrees,
        common.anchor,
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
    fps: u32,
    seed: u64,
    params: &Parameters,
    modulators: &ModulatorMap,
    layer: &mut GpuLayer,
    frame_index: u32,
) -> Result<()> {
    let Some(state) = evaluate_layer_state(
        &layer.id,
        &layer.position,
        layer.position_x.as_ref(),
        layer.position_y.as_ref(),
        &layer.scale,
        &layer.rotation_degrees,
        &layer.opacity,
        layer.timing,
        &layer.modulators,
        &layer.group_chain,
        frame_index,
        fps,
        params,
        seed,
        modulators,
    )?
    else {
        let hidden = LayerUniform {
            opacity: 0.0,
            _pad: [0.0; 3],
        };
        queue.write_buffer(&layer.uniform_buffer, 0, bytemuck::bytes_of(&hidden));
        layer.last_opacity = Some(0.0);
        return Ok(());
    };
    let opacity = state.opacity.clamp(0.0, 1.0);
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

    let vertices = build_layer_quad(
        frame_width,
        frame_height,
        layer.width,
        layer.height,
        state.position,
        state.scale,
        state.rotation_degrees,
        layer.anchor,
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
            p0: [0.0; 2],
            p1: [0.0; 2],
            p2: [0.0; 2],
            radius: 0.0,
            _pad_radius: [0.0; 5],
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
                p0: [0.0; 2],
                p1: [0.0; 2],
                p2: [0.0; 2],
                radius: 0.0,
                _pad_radius: [0.0; 5],
            }
        }
        ProceduralSource::Triangle { p0, p1, p2, color } => ProceduralUniform {
            kind: 2,
            axis: 0,
            _padding: [0; 2],
            color_a: color.as_array(),
            color_b: color.as_array(),
            p0: [p0.x, p0.y],
            p1: [p1.x, p1.y],
            p2: [p2.x, p2.y],
            radius: 0.0,
            _pad_radius: [0.0; 5],
        },
        ProceduralSource::Circle { center, radius, color } => ProceduralUniform {
            kind: 3,
            axis: 0,
            _padding: [0; 2],
            color_a: color.as_array(),
            color_b: color.as_array(),
            p0: [center.x, center.y],
            p1: [0.0; 2],
            p2: [0.0; 2],
            radius: *radius,
            _pad_radius: [0.0; 5],
        },
    }
}

#[derive(Debug, Clone, Copy)]
struct EvaluatedLayerState {
    position: Vec2,
    scale: Vec2,
    rotation_degrees: f32,
    opacity: f32,
}

fn resolve_group_chain(
    common: &LayerCommon,
    groups_by_id: &BTreeMap<String, Group>,
) -> Result<Vec<Group>> {
    let Some(group_id) = common.group.as_deref() else {
        return Ok(Vec::new());
    };

    let mut chain = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(group_id);
    while let Some(group_name) = current {
        if !seen.insert(group_name.to_owned()) {
            return Err(anyhow!(
                "layer '{}' has a cyclic group chain around '{}'",
                common.id,
                group_name
            ));
        }

        let group = groups_by_id.get(group_name).ok_or_else(|| {
            anyhow!(
                "layer '{}' references unknown group '{}'",
                common.id,
                group_name
            )
        })?;
        chain.push(group.clone());
        current = group.parent.as_deref();
    }

    chain.reverse();
    Ok(chain)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_layer_state(
    layer_id: &str,
    position: &PropertyValue<Vec2>,
    position_x: Option<&ScalarProperty>,
    position_y: Option<&ScalarProperty>,
    scale: &PropertyValue<Vec2>,
    rotation_degrees: &ScalarProperty,
    opacity: &ScalarProperty,
    timing: TimingControls,
    layer_modulators: &[ModulatorBinding],
    group_chain: &[Group],
    frame_index: u32,
    fps: u32,
    params: &Parameters,
    seed: u64,
    modulator_defs: &ModulatorMap,
) -> Result<Option<EvaluatedLayerState>> {
    let mut frame = frame_index as f32;
    let mut combined_position = Vec2 { x: 0.0, y: 0.0 };
    let mut combined_scale = Vec2 { x: 1.0, y: 1.0 };
    let mut combined_rotation = 0.0;
    let mut combined_opacity = 1.0;

    for group in group_chain {
        frame = match group.timing_controls().remap_frame(frame, fps) {
            Some(mapped) => mapped,
            None => return Ok(None),
        };

        let context = ExpressionContext::new(frame, params, seed);
        let mut group_position = group
            .sample_position_with_context(frame, &context)
            .with_context(|| format!("group '{}' failed to evaluate position", group.id))?;
        let mut group_scale = group.scale.sample_at(frame);
        let mut group_rotation = group
            .rotation_degrees
            .evaluate_with_context(&context)
            .with_context(|| format!("group '{}' failed to evaluate rotation", group.id))?;
        let mut group_opacity = group
            .opacity
            .evaluate_with_context(&context)
            .with_context(|| format!("group '{}' failed to evaluate opacity", group.id))?;

        apply_modulators(
            &group.modulators,
            &context,
            modulator_defs,
            &mut group_position,
            &mut group_scale,
            &mut group_rotation,
            &mut group_opacity,
            &format!("group '{}'", group.id),
        )?;

        combined_position.x += group_position.x;
        combined_position.y += group_position.y;
        combined_scale.x *= group_scale.x;
        combined_scale.y *= group_scale.y;
        combined_rotation += group_rotation;
        combined_opacity *= group_opacity;
    }

    frame = match timing.remap_frame(frame, fps) {
        Some(mapped) => mapped,
        None => return Ok(None),
    };
    let context = ExpressionContext::new(frame, params, seed);

    let mut layer_position = position.sample_at(frame);
    if let Some(x) = position_x {
        layer_position.x = x.evaluate_with_context(&context)?;
    }
    if let Some(y) = position_y {
        layer_position.y = y.evaluate_with_context(&context)?;
    }
    let mut layer_scale = scale.sample_at(frame);
    let mut layer_rotation = rotation_degrees.evaluate_with_context(&context)?;
    let mut layer_opacity = opacity.evaluate_with_context(&context)?;

    apply_modulators(
        layer_modulators,
        &context,
        modulator_defs,
        &mut layer_position,
        &mut layer_scale,
        &mut layer_rotation,
        &mut layer_opacity,
        &format!("layer '{layer_id}'"),
    )?;

    combined_position.x += layer_position.x;
    combined_position.y += layer_position.y;
    combined_scale.x *= layer_scale.x;
    combined_scale.y *= layer_scale.y;
    combined_rotation += layer_rotation;
    combined_opacity *= layer_opacity;

    if !combined_position.x.is_finite()
        || !combined_position.y.is_finite()
        || !combined_scale.x.is_finite()
        || !combined_scale.y.is_finite()
        || !combined_rotation.is_finite()
        || !combined_opacity.is_finite()
    {
        bail!("layer '{layer_id}' produced non-finite animation values");
    }

    if combined_opacity <= 0.0 {
        return Ok(None);
    }

    Ok(Some(EvaluatedLayerState {
        position: combined_position,
        scale: combined_scale,
        rotation_degrees: combined_rotation,
        opacity: combined_opacity.clamp(0.0, 1.0),
    }))
}

#[allow(clippy::too_many_arguments)]
fn apply_modulators(
    bindings: &[ModulatorBinding],
    context: &ExpressionContext<'_>,
    definitions: &ModulatorMap,
    position: &mut Vec2,
    scale: &mut Vec2,
    rotation_degrees: &mut f32,
    opacity: &mut f32,
    label: &str,
) -> Result<()> {
    for binding in bindings {
        let definition = definitions.get(&binding.source).ok_or_else(|| {
            anyhow!(
                "{label} references missing modulator '{}'; run `vcr lint` to diagnose",
                binding.source
            )
        })?;
        let value = definition
            .expression
            .evaluate_with_context(context)
            .with_context(|| format!("{label} failed evaluating modulator '{}'", binding.source))?;
        let weights = binding.weights;
        position.x += value * weights.x;
        position.y += value * weights.y;
        scale.x += value * weights.scale_x;
        scale.y += value * weights.scale_y;
        *rotation_degrees += value * weights.rotation;
        *opacity += value * weights.opacity;
    }
    Ok(())
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
    anchor: Anchor,
) -> [Vertex; 6] {
    let scaled_width = layer_width as f32 * scale.x.max(0.0);
    let scaled_height = layer_height as f32 * scale.y.max(0.0);

    let half_w = scaled_width * 0.5;
    let half_h = scaled_height * 0.5;

    let (center_x, center_y) = match anchor {
        Anchor::TopLeft => (position.x + half_w, position.y + half_h),
        Anchor::Center => (position.x, position.y),
    };

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

fn load_rgba_image(image_path: &Path, layer_id: &str) -> Result<image::RgbaImage> {
    let image = ImageReader::open(image_path)
        .with_context(|| format!("layer '{layer_id}': failed opening {}", image_path.display()))?
        .decode()
        .with_context(|| format!("layer '{layer_id}': failed decoding {}", image_path.display()))?;
    Ok(image.to_rgba8())
}

fn load_asset_pixmap(layer: &AssetLayer) -> Result<Pixmap> {
    load_layer_pixmap(&layer.source_path, &layer.common.id)
}

fn load_image_pixmap(layer: &ImageLayer) -> Result<Pixmap> {
    load_layer_pixmap(&layer.image.path, &layer.common.id)
}

fn load_layer_pixmap(image_path: &Path, layer_id: &str) -> Result<Pixmap> {
    let image = load_rgba_image(image_path, layer_id)?;
    let (width, height) = image.dimensions();

    let mut rgba = image.into_raw();
    premultiply_rgba_in_place(&mut rgba);

    let mut pixmap =
        Pixmap::new(width, height).ok_or_else(|| anyhow!("failed to allocate software pixmap for '{}'", layer_id))?;
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
    anchor: Anchor,
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

    let (center_x, center_y) = match anchor {
        Anchor::TopLeft => (position.x + half_w * scale_x, position.y + half_h * scale_y),
        Anchor::Center => (position.x, position.y),
    };

    let tx = center_x - (a * half_w + c * half_h);
    let ty = center_y - (b * half_w + d * half_h);

    Transform::from_row(a, b, c, d, tx, ty)
}

fn render_procedural_pixmap(source: &ProceduralSource, width: u32, height: u32) -> Result<Pixmap> {
    let mut pixmap = Pixmap::new(width, height).unwrap();
    pixmap.fill(Color::TRANSPARENT);

    match source {
        ProceduralSource::SolidColor { color } => {
            pixmap.fill(color_to_skia(*color));
        }
        ProceduralSource::Gradient {
            start_color,
            end_color,
            direction,
        } => {
            let mut paint = Paint::default();
            let (start, end) = match direction {
                GradientDirection::Horizontal => (Point::from_xy(0.0, 0.0), Point::from_xy(width as f32, 0.0)),
                GradientDirection::Vertical => (Point::from_xy(0.0, 0.0), Point::from_xy(0.0, height as f32)),
            };
            paint.shader = LinearGradient::new(
                start,
                end,
                vec![
                    GradientStop::new(0.0, color_to_skia(*start_color)),
                    GradientStop::new(1.0, color_to_skia(*end_color)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
            .ok_or_else(|| anyhow!("failed to create gradient"))?;
            pixmap.fill_rect(
                Rect::from_xywh(0.0, 0.0, width as f32, height as f32).unwrap(),
                &paint,
                Transform::identity(),
                None,
            );
        }
        ProceduralSource::Triangle { p0, p1, p2, color } => {
            let mut path = PathBuilder::new();
            path.move_to(p0.x * width as f32, p0.y * height as f32);
            path.line_to(p1.x * width as f32, p1.y * height as f32);
            path.line_to(p2.x * width as f32, p2.y * height as f32);
            path.close();

            let path = path.finish().ok_or_else(|| anyhow!("failed to create triangle path"))?;
            let mut paint = Paint::default();
            paint.set_color(color_to_skia(*color));
            paint.anti_alias = false;

            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
        ProceduralSource::Circle { center, radius, color } => {
            let mut path = PathBuilder::new();
            path.push_circle(center.x * width as f32, center.y * height as f32, radius * width as f32);
            
            let path = path.finish().ok_or_else(|| anyhow!("failed to create circle path"))?;
            let mut paint = Paint::default();
            paint.set_color(color_to_skia(*color));
            paint.anti_alias = false;

            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
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
fn render_text_to_pixmap(layer: &TextLayer) -> Result<Pixmap> {
    let font_path = match layer.text.font_family.to_lowercase().as_str() {
        "geistpixel-line" | "line" => "/Users/coltonbatts/Library/Fonts/GeistPixel-Line.ttf",
        "geistpixel-square" | "square" => "/Users/coltonbatts/Library/Fonts/GeistPixel-Square.ttf",
        "geistpixel-grid" | "grid" => "/Users/coltonbatts/Library/Fonts/GeistPixel-Grid.ttf",
        "geistpixel-circle" | "circle" => "/Users/coltonbatts/Library/Fonts/GeistPixel-Circle.ttf",
        "geistpixel-triangle" | "triangle" => "/Users/coltonbatts/Library/Fonts/GeistPixel-Triangle.ttf",
        _ => "/Users/coltonbatts/Library/Fonts/GeistPixel-Line.ttf",
    };

    let font_data = std::fs::read(font_path)
        .with_context(|| format!("failed to read font file {}", font_path))?;
    let font = fontdue::Font::from_bytes(font_data, fontdue::FontSettings::default())
        .map_err(|e| anyhow!("failed to parse font: {}", e))?;

    let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
    layout.reset(&fontdue::layout::LayoutSettings {
        x: 0.0,
        y: 0.0,
        max_width: None,
        max_height: None,
        horizontal_align: fontdue::layout::HorizontalAlign::Left,
        vertical_align: fontdue::layout::VerticalAlign::Top,
        line_height: 1.0,
        wrap_style: fontdue::layout::WrapStyle::Letter,
        wrap_hard_breaks: true,
    });

    layout.append(&[&font], &fontdue::layout::TextStyle::new(&layer.text.content, layer.text.font_size, 0));

    let glyphs = layout.glyphs();
    if glyphs.is_empty() {
        return Ok(Pixmap::new(1, 1).unwrap());
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for glyph in glyphs {
        min_x = min_x.min(glyph.x);
        min_y = min_y.min(glyph.y);
        max_x = max_x.max(glyph.x + glyph.width as f32);
        max_y = max_y.max(glyph.y + glyph.height as f32);
    }

    let width = (max_x - min_x).ceil() as u32;
    let height = (max_y - min_y).ceil() as u32;

    let mut pixmap = Pixmap::new(width.max(1), height.max(1))
        .ok_or_else(|| anyhow!("failed to allocate pixmap for text render"))?;
    pixmap.fill(Color::TRANSPARENT);

    let color = layer.text.color;
    let r = f32_to_channel(color.r);
    let g = f32_to_channel(color.g);
    let b = f32_to_channel(color.b);
    let a_base = color.a;

    for glyph in glyphs {
        if glyph.width == 0 || glyph.height == 0 {
            continue;
        }
        let (_, bitmap) = font.rasterize_config(glyph.key);
        for row in 0..glyph.height {
            for col in 0..glyph.width {
                let alpha_mask = bitmap[row * glyph.width + col] as f32 / 255.0;
                let alpha = (a_base * alpha_mask * 255.0).round() as u8;
                if alpha == 0 { continue; }
                
                let x = (glyph.x - min_x) as u32 + col as u32;
                let y = (glyph.y - min_y) as u32 + row as u32;
                
                if x < width && y < height {
                    let index = (y * width + x) as usize;
                    if let Some(pixel) = pixmap.pixels_mut().get_mut(index) {
                        let pr = ((r as u16 * alpha as u16 + 127) / 255) as u8;
                        let pg = ((g as u16 * alpha as u16 + 127) / 255) as u8;
                        let pb = ((b as u16 * alpha as u16 + 127) / 255) as u8;

                        // Simple alpha blend (over)
                        let dst_a = pixel.alpha() as f32 / 255.0;
                        let src_a = alpha as f32 / 255.0;
                        let out_a = src_a + dst_a * (1.0 - src_a);
                        
                        if out_a > 0.0 {
                            let out_r = (pr as f32 + pixel.red() as f32 * (1.0 - src_a)).min(255.0);
                            let out_g = (pg as f32 + pixel.green() as f32 * (1.0 - src_a)).min(255.0);
                            let out_b = (pb as f32 + pixel.blue() as f32 * (1.0 - src_a)).min(255.0);
                            *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                                out_r as u8,
                                out_g as u8,
                                out_b as u8,
                                (out_a * 255.0).round() as u8,
                            ).expect("premultiplied color should be valid");
                        }
                    }
                }
            }
        }
    }

    Ok(pixmap)
}

fn build_text_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    layer: &TextLayer,
    group_chain: Vec<Group>,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    groups_by_id: &BTreeMap<String, Group>,
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64,
    fps: u32,
) -> Result<GpuLayer> {
    let pixmap = render_text_to_pixmap(layer)?;
    let (layer_width, layer_height) = (pixmap.width(), pixmap.height());

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("vcr-text-layer-{}", layer.common.id)),
        size: wgpu::Extent3d {
            width: layer_width,
            height: layer_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixmap.data(),
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(layer_width * 4),
            rows_per_image: Some(layer_height),
        },
        wgpu::Extent3d {
            width: layer_width,
            height: layer_height,
            depth_or_array_layers: 1,
        },
    );

    let state = evaluate_layer_state(
        &layer.common.id,
        &layer.common.position,
        layer.common.pos_x.as_ref(),
        layer.common.pos_y.as_ref(),
        &layer.common.scale,
        &layer.common.rotation_degrees,
        &layer.common.opacity,
        layer.common.timing_controls(),
        &layer.common.modulators,
        &group_chain,
        0,
        fps,
        params,
        seed,
        modulators,
    )?.ok_or_else(|| anyhow!("layer not active at frame 0"))?;

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        state.position,
        state.scale,
        state.rotation_degrees,
        layer_width,
        layer_height,
        &texture_view,
        blend_bind_group_layout,
        sampler,
    )?;

    Ok(GpuLayer {
        id: layer.common.id.clone(),
        z_index: layer.common.z_index,
        width: layer_width,
        height: layer_height,
        position: layer.common.position.clone(),
        position_x: layer.common.pos_x.clone(),
        position_y: layer.common.pos_y.clone(),
        scale: layer.common.scale.clone(),
        rotation_degrees: layer.common.rotation_degrees.clone(),
        opacity: layer.common.opacity.clone(),
        timing: layer.common.timing_controls(),
        modulators: layer.common.modulators.clone(),
        all_properties_static: layer.common.has_static_properties()
            && group_chain.iter().all(Group::has_static_properties),
        group_chain, anchor: layer.common.anchor,
        uniform_buffer: draw_resources.uniform_buffer,
        blend_bind_group: draw_resources.blend_bind_group,
        vertex_buffer: draw_resources.vertex_buffer,
        last_vertices: Some(draw_resources.initial_vertices),
        last_opacity: Some(draw_resources.initial_opacity),
        source: GpuLayerSource::Text { _texture: texture },
    })
}

#[cfg(test)]
mod tests {
    use super::{copy_tight_rows, RenderSceneData, SoftwareRenderer};
    use crate::schema::Manifest;

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

    #[test]
    fn software_renderer_golden_checksum_is_stable() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 8 }
seed: 42
params:
  energy: 0.8
modulators:
  wobble:
    expression: "noise1d(t * 0.3) * energy"
groups:
  - id: rig
    position: [2, 2]
    modulators:
      - source: wobble
        weights:
          x: 1.5
layers:
  - id: background
    procedural:
      kind: solid_color
      color: { r: 0.1, g: 0.1, b: 0.1, a: 1.0 }
  - id: accent
    group: rig
    position: [1, 1]
    scale: [0.8, 0.8]
    rotation_degrees: "sin(t * 0.2) * 12"
    opacity: "clamp(0.6 + env(t, 2, 6) * 0.4, 0, 1)"
    modulators:
      - source: wobble
        weights:
          y: 1.0
          rotation: 4.0
    procedural:
      kind: gradient
      start_color: { r: 0.9, g: 0.2, b: 0.1, a: 0.9 }
      end_color: { r: 0.1, g: 0.4, b: 0.9, a: 0.9 }
      direction: horizontal
"#,
        )
        .expect("manifest should parse");

        let mut renderer = SoftwareRenderer::new(
            &manifest.environment,
            &manifest.layers,
            &RenderSceneData::from_manifest(&manifest),
        )
        .expect("software renderer should initialize");

        let frame = renderer
            .render_frame_rgba(4)
            .expect("frame render should succeed");
        let checksum = fnv1a64(&frame);
        assert_eq!(checksum, 2991149225877046887);
    }

    fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
}
