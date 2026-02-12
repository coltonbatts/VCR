use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, PathBuilder,
    Pixmap, PixmapPaint, Point, Rect, SpreadMode, Transform,
};
use wgpu::util::DeviceExt;

use crate::ascii::PreparedAsciiLayer;
use crate::ascii_pipeline::AsciiPipeline;
use crate::post_process::PostStack;
use crate::schema::{
    Anchor, AnimatableColor, AsciiLayer, AssetLayer, ColorRgba, Environment, ExpressionContext,
    GradientDirection, Group, ImageLayer, Layer, LayerCommon, ModulatorBinding, ModulatorMap,
    Parameters, ProceduralLayer, ProceduralSource, PropertyValue, ScalarProperty, ShaderLayer,
    TextLayer, TimingControls, Vec2,
};
use crate::timeline::{
    evaluate_layer_state, evaluate_layer_state_or_hidden, resolve_group_chain,
    resolve_groups_by_id, RenderSceneData,
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
  extra_u32: u32,
  _padding: u32,
  color_a: vec4<f32>,
  color_b: vec4<f32>,
  p0: vec2<f32>,
  p1: vec2<f32>,
  p2: vec2<f32>,
  radius: f32,
  inner_radius: f32,
  corner_radius: f32,
  thickness: f32,
  size: vec2<f32>,
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

const PI: f32 = 3.14159265358979;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let uv = clamp(input.uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
  let transparent = vec4<f32>(0.0, 0.0, 0.0, 0.0);

  // 0: SolidColor
  if procedural.kind == 0u {
    return procedural.color_a;
  }

  // 1: Gradient
  if procedural.kind == 1u {
    let amount = select(uv.y, uv.x, procedural.axis == 0u);
    return mix(procedural.color_a, procedural.color_b, amount);
  }

  // 2: Triangle
  if procedural.kind == 2u {
    let d1 = sign_func(uv, procedural.p0, procedural.p1);
    let d2 = sign_func(uv, procedural.p1, procedural.p2);
    let d3 = sign_func(uv, procedural.p2, procedural.p0);

    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);

    if !(has_neg && has_pos) {
      return procedural.color_a;
    }
    return transparent;
  }

  // 3: Circle
  if procedural.kind == 3u {
    let dist = distance(uv, procedural.p0);
    if dist < procedural.radius {
      return procedural.color_a;
    }
    return transparent;
  }

  // 4: RoundedRect
  if procedural.kind == 4u {
    let half_size = procedural.size * 0.5;
    let d = abs(uv - procedural.p0) - half_size + vec2<f32>(procedural.corner_radius);
    let sdf = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - procedural.corner_radius;
    if sdf <= 0.0 {
      return procedural.color_a;
    }
    return transparent;
  }

  // 5: Ring
  if procedural.kind == 5u {
    let dist = distance(uv, procedural.p0);
    if dist <= procedural.radius && dist >= procedural.inner_radius {
      return procedural.color_a;
    }
    return transparent;
  }

  // 6: Line (capsule SDF)
  if procedural.kind == 6u {
    let ab = procedural.p1 - procedural.p0;
    let ap = uv - procedural.p0;
    let t_line = clamp(dot(ap, ab) / dot(ab, ab), 0.0, 1.0);
    let closest = procedural.p0 + ab * t_line;
    let dist = distance(uv, closest);
    if dist <= procedural.thickness * 0.5 {
      return procedural.color_a;
    }
    return transparent;
  }

  // 7: Polygon (regular n-gon SDF)
  if procedural.kind == 7u {
    let n = f32(procedural.extra_u32);
    let p = uv - procedural.p0;
    let angle = atan2(p.y, p.x);
    let sector = 2.0 * PI / n;
    let r = length(p);
    let theta = ((angle % sector) + sector) % sector;
    let half_sector = sector * 0.5;
    let cos_half = cos(half_sector);
    let edge_dist = procedural.radius * cos_half;
    let proj = r * cos(theta - half_sector);
    if proj <= edge_dist {
      return procedural.color_a;
    }
    return transparent;
  }

  // Default fallback
  return transparent;
}
"#;

const CUSTOM_SHADER_PREAMBLE: &str = r#"
struct ShaderUniforms {
  time: f32,
  frame: u32,
  resolution: vec2<f32>,
  custom: array<vec4<f32>, 2>,
}

@group(0) @binding(0) var<uniform> vcr_uniforms: ShaderUniforms;

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
  return shade(input.uv, vcr_uniforms);
}
"#;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ShaderUniform {
    time: f32,
    frame: u32,
    resolution: [f32; 2],
    custom: [f32; 8],
}

const EPSILON: f32 = 0.0001;
const NO_GPU_ADAPTER_ERR: &str = "no suitable GPU adapter found";
const READBACK_BUFFER_COUNT: usize = 2;
const READBACK_MAP_TIMEOUT: Duration = Duration::from_secs(5);
const READBACK_POLL_INTERVAL: Duration = Duration::from_millis(1);

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
    extra_u32: u32, // polygon sides
    _padding: u32,
    color_a: [f32; 4],
    color_b: [f32; 4],
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    radius: f32,
    inner_radius: f32,
    corner_radius: f32,
    thickness: f32,
    size: [f32; 2],
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
            GpuLayerSource::Shader(gpu) => gpu.has_rendered && gpu.is_static,
            GpuLayerSource::Text { .. } => true,
            GpuLayerSource::Ascii(gpu) => gpu.has_rendered && gpu.is_static,
        }
    }
}

enum GpuLayerSource {
    Asset { _texture: wgpu::Texture },
    Procedural(ProceduralGpu),
    Shader(CustomShaderGpu),
    Text { _texture: wgpu::Texture },
    Ascii(AsciiGpu),
}

struct CustomShaderGpu {
    uniforms: Vec<ScalarProperty>,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    has_rendered: bool,
    is_static: bool,
    last_rendered_frame: Option<u32>,
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

struct AsciiGpu {
    prepared: PreparedAsciiLayer,
    texture: wgpu::Texture,
    has_rendered: bool,
    is_static: bool,
    last_rendered_frame: Option<u32>,
}

struct GpuRenderer {
    adapter_name: String,
    adapter_backend: wgpu::Backend,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    width: u32,
    height: u32,
    fps: u32,
    seed: u64,
    params: Parameters,
    modulators: ModulatorMap,
    output_texture: wgpu::Texture,
    readback_buffers: [wgpu::Buffer; READBACK_BUFFER_COUNT],
    next_readback_index: usize,
    pending_readback: Option<PendingReadback>,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    blend_pipeline: wgpu::RenderPipeline,
    procedural_pipeline: wgpu::RenderPipeline,
    layers: Vec<GpuLayer>,
    post_stack: PostStack,
    ascii_pipeline: Option<AsciiPipeline>,
}

struct PendingReadback {
    buffer_index: usize,
    submission_index: wgpu::SubmissionIndex,
}

#[cfg(target_os = "macos")]
const PREFERRED_BACKENDS: wgpu::Backends = wgpu::Backends::METAL;

#[cfg(not(target_os = "macos"))]
const PREFERRED_BACKENDS: wgpu::Backends = wgpu::Backends::PRIMARY;

pub struct RendererGpuContext {
    _instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub adapter_name: String,
    pub adapter_backend: wgpu::Backend,
}

impl RendererGpuContext {
    pub async fn headless() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: PREFERRED_BACKENDS,
            ..Default::default()
        });
        Self::new_with_instance(instance, None).await
    }

    pub async fn for_surface(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
    ) -> Result<Self> {
        Self::new_with_instance(instance, Some(surface)).await
    }

    async fn new_with_instance(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self> {
        let adapter = request_best_adapter(&instance, surface).await?;
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

        Ok(Self {
            _instance: instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_name: adapter_info.name,
            adapter_backend: adapter_info.backend,
        })
    }
}

async fn request_best_adapter(
    instance: &wgpu::Instance,
    compatible_surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter> {
    if let Some(adapter) = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface,
        })
        .await
    {
        return Ok(adapter);
    }

    if let Some(adapter) = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface,
        })
        .await
    {
        return Ok(adapter);
    }

    if let Some(adapter) = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: true,
            compatible_surface,
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
    Shader,
    Text { pixmap: Pixmap },
    Ascii { prepared: PreparedAsciiLayer },
}

impl GpuRenderer {
    pub async fn new(
        environment: &Environment,
        layers: &[Layer],
        scene: &RenderSceneData,
    ) -> Result<Self> {
        let context = RendererGpuContext::headless().await?;
        Self::new_with_context(
            environment,
            layers,
            scene,
            &context,
            wgpu::TextureFormat::Rgba8Unorm,
        )
    }

    pub fn new_with_context(
        environment: &Environment,
        layers: &[Layer],
        scene: &RenderSceneData,
        context: &RendererGpuContext,
        render_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let width = environment.resolution.width;
        let height = environment.resolution.height;
        let groups_by_id = resolve_groups_by_id(&scene.groups);
        let device = context.device.clone();
        let queue = context.queue.clone();

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
            format: render_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let unpadded_bytes_per_row = checked_bytes_per_row(width, "frame width")?.get();
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                .context("failed to align frame row bytes for GPU readback")?;
        let readback_size = frame_size_bytes_u64(padded_bytes_per_row, height)
            .context("failed to compute GPU readback buffer size")?;
        let readback_buffers = [
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("vcr-readback-buffer-0"),
                size: readback_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("vcr-readback-buffer-1"),
                size: readback_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
        ];

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
                    format: render_format,
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
                    &scene.params,
                    &scene.modulators,
                    scene.seed,
                    environment.fps,
                )?,
                Layer::Shader(shader_layer) => build_shader_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    shader_layer,
                    group_chain,
                    &blend_bind_group_layout,
                    &sampler,
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
                    &scene.params,
                    &scene.modulators,
                    scene.seed,
                    environment.fps,
                )?,
                Layer::Ascii(ascii_layer) => build_ascii_layer(
                    &device,
                    &queue,
                    width,
                    height,
                    ascii_layer,
                    group_chain,
                    &blend_bind_group_layout,
                    &sampler,
                    &scene.params,
                    &scene.modulators,
                    scene.seed,
                    environment.fps,
                )?,
            };
            gpu_layers.push(gpu_layer);
        }
        gpu_layers.sort_by_key(|layer| layer.z_index);

        let post_stack = PostStack::new(&device, width, height, render_format, &scene.post)
            .context("failed to initialize post-processing stack")?;

        let ascii_pipeline = match &scene.ascii_post {
            Some(config) if config.enabled => {
                let enable_edge_boost = scene
                    .ascii_overrides
                    .as_ref()
                    .and_then(|o| o.edge_boost)
                    .unwrap_or(crate::ascii_pipeline::DEFAULT_EDGE_BOOST);
                let enable_bayer_dither = scene
                    .ascii_overrides
                    .as_ref()
                    .and_then(|o| o.bayer_dither)
                    .unwrap_or(crate::ascii_pipeline::DEFAULT_BAYER_DITHER);
                let pipeline = AsciiPipeline::new(
                    &device,
                    &queue,
                    config,
                    width,
                    height,
                    render_format,
                    enable_edge_boost,
                    enable_bayer_dither,
                )
                .context("failed to initialize ASCII post-processing pipeline")?;
                Some(pipeline)
            }
            _ => None,
        };

        Ok(Self {
            adapter_name: context.adapter_name.clone(),
            adapter_backend: context.adapter_backend,
            device,
            queue,
            width,
            height,
            fps: environment.fps,
            seed: scene.seed,
            params: scene.params.clone(),
            modulators: scene.modulators.clone(),
            output_texture,
            readback_buffers,
            next_readback_index: 0,
            pending_readback: None,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            blend_pipeline,
            procedural_pipeline,
            layers: gpu_layers,
            post_stack,
            ascii_pipeline,
        })
    }

    pub fn render_frame(&mut self, frame_index: u32) -> Result<()> {
        let output_view = self
            .output_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let readback_index = self.next_readback_index;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("vcr-render-encoder"),
            });

        self.prepare_procedural_layers(frame_index, &mut encoder)?;
        self.render_layers_to_view(frame_index, &output_view, &mut encoder)?;

        // Apply post-processing stack (if any effects are configured)
        if !self.post_stack.is_empty() {
            self.post_stack.apply(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.output_texture,
                frame_index,
                self.fps,
                self.seed,
            );
        }

        // Apply ASCII post-processing pipeline (if enabled)
        if let Some(ascii) = &self.ascii_pipeline {
            ascii.apply(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.output_texture,
                frame_index,
                self.fps,
                self.seed,
            );
            ascii.copy_debug_to_output(&mut encoder, &self.output_texture);
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
                buffer: &self.readback_buffers[readback_index],
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

        let submission_index = self.queue.submit(Some(encoder.finish()));
        self.pending_readback = Some(PendingReadback {
            buffer_index: readback_index,
            submission_index,
        });
        self.next_readback_index = (self.next_readback_index + 1) % self.readback_buffers.len();
        Ok(())
    }

    pub fn render_frame_to_view(
        &mut self,
        frame_index: u32,
        target_view: &wgpu::TextureView,
    ) -> Result<()> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("vcr-preview-encoder"),
            });

        self.prepare_procedural_layers(frame_index, &mut encoder)?;
        self.render_layers_to_view(frame_index, target_view, &mut encoder)?;

        // Apply post-processing for live preview path.
        // Note: for render_frame_to_view we write directly to the provided target_view,
        // so post-processing would need its own intermediate. For now, post-processing
        // is only applied in the headless render path (render_frame). This keeps the
        // preview path simple and avoids extra texture copies. A future enhancement
        // could extend this.

        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn render_layers_to_view(
        &mut self,
        frame_index: u32,
        target_view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<()> {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vcr-render-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
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

        Ok(())
    }

    pub fn read_buffer(&mut self) -> Result<Vec<u8>> {
        let pending = self
            .pending_readback
            .take()
            .ok_or_else(|| anyhow!("readback requested before any rendered frame was submitted"))?;
        let buffer_slice = self.readback_buffers[pending.buffer_index].slice(..);
        let (sender, receiver) = mpsc::channel();

        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let start = Instant::now();
        let map_result = loop {
            match receiver.try_recv() {
                Ok(result) => break result,
                Err(mpsc::TryRecvError::Empty) => {
                    if start.elapsed() >= READBACK_MAP_TIMEOUT {
                        return Err(anyhow!(
                            "timed out waiting for GPU readback (submission {:?})",
                            pending.submission_index
                        ));
                    }
                    self.device.poll(wgpu::Maintain::Poll);
                    thread::sleep(READBACK_POLL_INTERVAL);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(anyhow!("failed receiving GPU map callback"));
                }
            }
        };

        map_result.context("GPU buffer mapping failed")?;

        let mapped = buffer_slice.get_mapped_range();
        let frame = copy_tight_rows(
            &mapped,
            self.unpadded_bytes_per_row,
            self.padded_bytes_per_row,
            self.height,
        )?;

        drop(mapped);
        self.readback_buffers[pending.buffer_index].unmap();
        Ok(frame)
    }

    pub fn render_frame_rgba(&mut self, frame_index: u32) -> Result<Vec<u8>> {
        self.render_frame(frame_index)?;
        self.read_buffer()
    }

    fn prepare_procedural_layers(
        &mut self,
        frame_index: u32,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<()> {
        let context = ExpressionContext::new(frame_index as f32, &self.params, self.seed);
        for layer in &mut self.layers {
            let GpuLayerSource::Procedural(procedural) = &mut layer.source else {
                continue;
            };

            let needs_update = !procedural.has_rendered || !procedural.is_static;
            if !needs_update {
                continue;
            }

            let uniform = evaluate_procedural_uniform(&procedural.source, &context)?;
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

        // Shader layers
        let fps = self.fps;
        for layer in &mut self.layers {
            let GpuLayerSource::Shader(shader) = &mut layer.source else {
                continue;
            };

            let needs_update = shader_layer_needs_update(shader.last_rendered_frame, frame_index);
            if !needs_update {
                continue;
            }

            let time = frame_index as f32 / fps as f32;
            let mut custom = [0.0_f32; 8];
            for (i, prop) in shader.uniforms.iter().enumerate() {
                custom[i] = prop.evaluate_with_context(&context)?;
            }
            let uniform = ShaderUniform {
                time,
                frame: frame_index,
                resolution: [self.width as f32, self.height as f32],
                custom,
            };
            self.queue
                .write_buffer(&shader.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("vcr-shader-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &shader.view,
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
                pass.set_pipeline(&shader.pipeline);
                pass.set_bind_group(0, &shader.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }

            shader.has_rendered = true;
            shader.last_rendered_frame = Some(frame_index);
        }

        // ASCII layers
        for layer in &mut self.layers {
            let GpuLayerSource::Ascii(ascii) = &mut layer.source else {
                continue;
            };

            let needs_update = !ascii.has_rendered || !ascii.is_static;
            if !needs_update {
                continue;
            }

            let pixmap = ascii.prepared.render_frame_pixmap(frame_index)?;
            queue_write_pixmap_texture(
                &self.queue,
                &ascii.texture,
                pixmap.as_ref(),
                &format!("ascii layer '{}'", layer.id),
            )?;
            ascii.has_rendered = true;
            ascii.last_rendered_frame = Some(frame_index);
        }

        Ok(())
    }
}

impl Renderer {
    pub fn new_software(
        environment: &Environment,
        layers: &[Layer],
        scene: RenderSceneData,
    ) -> Result<Self> {
        let software = SoftwareRenderer::new(environment, layers, &scene)
            .context("failed to initialize software renderer")?;
        Ok(Self {
            backend: RendererBackend::Software(software),
            backend_reason: "forced software backend".to_owned(),
        })
    }

    pub async fn new_with_scene(
        environment: &Environment,
        layers: &[Layer],
        scene: RenderSceneData,
    ) -> Result<Self> {
        let gpu = match GpuRenderer::new(environment, layers, &scene).await {
            Ok(gpu) => gpu,
            Err(error) => {
                let error_message = error.to_string();
                if can_use_software_fallback(&error_message, layers) {
                    let software = SoftwareRenderer::new(environment, layers, &scene)
                        .context("failed to initialize software renderer fallback")?;
                    return Ok(Self {
                        backend: RendererBackend::Software(software),
                        backend_reason: error_message,
                    });
                }
                if error_message.contains(NO_GPU_ADAPTER_ERR) && has_shader_layers(layers) {
                    return Err(error.context(
                        "software fallback is disabled because the manifest contains shader layers",
                    ));
                }
                return Err(error);
            }
        };

        Ok(Self {
            backend_reason: format!("adapter '{}' ({:?})", gpu.adapter_name, gpu.adapter_backend),
            backend: RendererBackend::Gpu(gpu),
        })
    }

    pub fn new_with_scene_and_context(
        environment: &Environment,
        layers: &[Layer],
        scene: RenderSceneData,
        context: &RendererGpuContext,
        render_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let gpu = match GpuRenderer::new_with_context(
            environment,
            layers,
            &scene,
            context,
            render_format,
        ) {
            Ok(gpu) => gpu,
            Err(error) => {
                let error_message = error.to_string();
                if can_use_software_fallback(&error_message, layers) {
                    let software = SoftwareRenderer::new(environment, layers, &scene)
                        .context("failed to initialize software renderer fallback")?;
                    return Ok(Self {
                        backend: RendererBackend::Software(software),
                        backend_reason: error_message,
                    });
                }
                return Err(error);
            }
        };

        Ok(Self {
            backend_reason: format!("adapter '{}' ({:?})", gpu.adapter_name, gpu.adapter_backend),
            backend: RendererBackend::Gpu(gpu),
        })
    }

    pub fn is_gpu_backend(&self) -> bool {
        matches!(self.backend, RendererBackend::Gpu(_))
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

    pub fn render_frame_to_view(
        &mut self,
        frame_index: u32,
        target_view: &wgpu::TextureView,
    ) -> Result<()> {
        match &mut self.backend {
            RendererBackend::Gpu(renderer) => {
                renderer.render_frame_to_view(frame_index, target_view)
            }
            RendererBackend::Software(_) => {
                bail!("direct rendering to window surface requires GPU backend")
            }
        }
    }
}

impl SoftwareRenderer {
    fn new(environment: &Environment, layers: &[Layer], scene: &RenderSceneData) -> Result<Self> {
        let width = environment.resolution.width;
        let height = environment.resolution.height;
        let groups_by_id = resolve_groups_by_id(&scene.groups);
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
                Layer::Shader(_) => {
                    eprintln!("[warn] custom shader layers require GPU backend; layer rendered as transparent");
                    SoftwareLayerSource::Shader
                }
                Layer::Text(text_layer) => SoftwareLayerSource::Text {
                    pixmap: render_text_to_pixmap(text_layer)?,
                },
                Layer::Ascii(ascii_layer) => SoftwareLayerSource::Ascii {
                    prepared: PreparedAsciiLayer::new(&ascii_layer.ascii, &ascii_layer.common.id)?,
                },
            };

            let (layer_width, layer_height) = match &source {
                SoftwareLayerSource::Asset { pixmap } => (pixmap.width(), pixmap.height()),
                SoftwareLayerSource::Procedural(_) => (width, height),
                SoftwareLayerSource::Shader => (width, height),
                SoftwareLayerSource::Text { pixmap } => (pixmap.width(), pixmap.height()),
                SoftwareLayerSource::Ascii { prepared } => {
                    (prepared.pixel_width(), prepared.pixel_height())
                }
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
                let context = ExpressionContext::new(frame_index as f32, &self.params, self.seed);
                let procedural =
                    render_procedural_pixmap(source, self.width, self.height, &context)?;
                draw_layer_pixmap(output, procedural.as_ref(), opacity, transform);
            }
            SoftwareLayerSource::Shader => {
                // Custom WGSL shaders can't run on CPU — skip
            }
            SoftwareLayerSource::Text { pixmap } => {
                draw_layer_pixmap(output, pixmap.as_ref(), opacity, transform);
            }
            SoftwareLayerSource::Ascii { prepared } => {
                let pixmap = prepared.render_frame_pixmap(frame_index)?;
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
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64,
    fps: u32,
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

    let bytes_per_row =
        checked_bytes_per_row(layer_width, &format!("layer '{}' width", common.id))?;
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

    let state = evaluate_layer_state_or_hidden(
        &common.id,
        &common.position,
        common.pos_x.as_ref(),
        common.pos_y.as_ref(),
        &common.scale,
        &common.rotation_degrees,
        &common.opacity,
        common.timing_controls(),
        &common.modulators,
        &group_chain,
        0,
        fps,
        params,
        seed,
        modulators,
    )?;

    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        common,
        state.position,
        state.scale,
        state.rotation_degrees,
        state.opacity,
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
        group_chain,
        anchor: common.anchor,
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

    let init_context = ExpressionContext::new(0.0, params, seed);
    let uniform = evaluate_procedural_uniform(&layer.procedural, &init_context)?;
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

    let state = evaluate_layer_state_or_hidden(
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
    )?;

    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        state.position,
        state.scale,
        state.rotation_degrees,
        state.opacity,
        frame_width,
        frame_height,
        &view,
        blend_bind_group_layout,
        sampler,
    )?;

    let procedural_gpu = ProceduralGpu {
        source: layer.procedural.clone(),
        is_static: layer.procedural.is_static(),
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
        group_chain,
        anchor: layer.common.anchor,
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
    initial_opacity: f32,
    layer_width: u32,
    layer_height: u32,
    sampled_texture_view: &wgpu::TextureView,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> Result<LayerDrawResources> {
    let opacity = initial_opacity.clamp(0.0, 1.0);
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

fn evaluate_procedural_uniform(
    source: &ProceduralSource,
    context: &ExpressionContext<'_>,
) -> Result<ProceduralUniform> {
    let default = ProceduralUniform {
        kind: 0,
        axis: 0,
        extra_u32: 0,
        _padding: 0,
        color_a: [0.0; 4],
        color_b: [0.0; 4],
        p0: [0.0; 2],
        p1: [0.0; 2],
        p2: [0.0; 2],
        radius: 0.0,
        inner_radius: 0.0,
        corner_radius: 0.0,
        thickness: 0.0,
        size: [0.0; 2],
    };

    fn eval_color(c: &AnimatableColor, ctx: &ExpressionContext<'_>) -> Result<[f32; 4]> {
        Ok(c.evaluate(ctx)?.as_array())
    }

    Ok(match source {
        ProceduralSource::SolidColor { color } => {
            let c = eval_color(color, context)?;
            ProceduralUniform {
                kind: 0,
                color_a: c,
                color_b: c,
                ..default
            }
        }
        ProceduralSource::Gradient {
            start_color,
            end_color,
            direction,
        } => ProceduralUniform {
            kind: 1,
            axis: match direction {
                GradientDirection::Horizontal => 0,
                GradientDirection::Vertical => 1,
            },
            color_a: eval_color(start_color, context)?,
            color_b: eval_color(end_color, context)?,
            ..default
        },
        ProceduralSource::Triangle { p0, p1, p2, color } => {
            let c = eval_color(color, context)?;
            ProceduralUniform {
                kind: 2,
                color_a: c,
                color_b: c,
                p0: [p0.x, p0.y],
                p1: [p1.x, p1.y],
                p2: [p2.x, p2.y],
                ..default
            }
        }
        ProceduralSource::Circle {
            center,
            radius,
            color,
        } => {
            let c = eval_color(color, context)?;
            ProceduralUniform {
                kind: 3,
                color_a: c,
                color_b: c,
                p0: [center.x, center.y],
                radius: radius.evaluate_with_context(context)?,
                ..default
            }
        }
        ProceduralSource::RoundedRect {
            center,
            size,
            corner_radius,
            color,
        } => ProceduralUniform {
            kind: 4,
            color_a: eval_color(color, context)?,
            p0: [center.x, center.y],
            size: [size.x, size.y],
            corner_radius: corner_radius.evaluate_with_context(context)?,
            ..default
        },
        ProceduralSource::Ring {
            center,
            outer_radius,
            inner_radius,
            color,
        } => ProceduralUniform {
            kind: 5,
            color_a: eval_color(color, context)?,
            p0: [center.x, center.y],
            radius: outer_radius.evaluate_with_context(context)?,
            inner_radius: inner_radius.evaluate_with_context(context)?,
            ..default
        },
        ProceduralSource::Line {
            start,
            end,
            thickness,
            color,
        } => ProceduralUniform {
            kind: 6,
            color_a: eval_color(color, context)?,
            p0: [start.x, start.y],
            p1: [end.x, end.y],
            thickness: thickness.evaluate_with_context(context)?,
            ..default
        },
        ProceduralSource::Polygon {
            center,
            radius,
            sides,
            color,
        } => ProceduralUniform {
            kind: 7,
            extra_u32: *sides,
            color_a: eval_color(color, context)?,
            p0: [center.x, center.y],
            radius: radius.evaluate_with_context(context)?,
            ..default
        },
    })
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

fn align_to(value: u32, alignment: u32) -> Result<u32> {
    if alignment == 0 {
        bail!("alignment must be non-zero");
    }
    if !alignment.is_power_of_two() {
        bail!("alignment must be a power of two, got {alignment}");
    }
    let mask = alignment - 1;
    value
        .checked_add(mask)
        .map(|aligned| aligned & !mask)
        .ok_or_else(|| anyhow!("overflow while aligning {value} to {alignment}"))
}

fn checked_bytes_per_row(width: u32, label: &str) -> Result<NonZeroU32> {
    let bytes_per_row = width
        .checked_mul(4)
        .ok_or_else(|| anyhow!("{label} overflows when computing bytes_per_row"))?;
    NonZeroU32::new(bytes_per_row).ok_or_else(|| anyhow!("{label} must be greater than zero"))
}

fn frame_size_bytes_u64(bytes_per_row: u32, height: u32) -> Result<u64> {
    u64::from(bytes_per_row)
        .checked_mul(u64::from(height))
        .ok_or_else(|| anyhow!("frame size overflow for {bytes_per_row}x{height} bytes"))
}

fn shader_layer_needs_update(last_rendered_frame: Option<u32>, frame_index: u32) -> bool {
    last_rendered_frame != Some(frame_index)
}

fn has_shader_layers(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| matches!(layer, Layer::Shader(_)))
}

fn can_use_software_fallback(gpu_error_message: &str, layers: &[Layer]) -> bool {
    gpu_error_message.contains(NO_GPU_ADAPTER_ERR) && !has_shader_layers(layers)
}

fn copy_tight_rows(
    mapped: &[u8],
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    height: u32,
) -> Result<Vec<u8>> {
    if unpadded_bytes_per_row > padded_bytes_per_row {
        bail!(
            "unpadded row size ({unpadded_bytes_per_row}) cannot exceed padded row size ({padded_bytes_per_row})"
        );
    }
    let height = usize::try_from(height).context("frame height does not fit platform usize")?;
    let unpadded_bytes_per_row = usize::try_from(unpadded_bytes_per_row)
        .context("unpadded row bytes do not fit platform usize")?;
    let padded_bytes_per_row = usize::try_from(padded_bytes_per_row)
        .context("padded row bytes do not fit platform usize")?;

    let required_len = padded_bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| anyhow!("mapped frame size overflow while validating row copy"))?;
    if mapped.len() < required_len {
        return Err(anyhow!(
            "mapped frame too small: expected at least {} bytes, got {}",
            required_len,
            mapped.len()
        ));
    }

    let frame_len = unpadded_bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| anyhow!("tight frame size overflow while copying mapped rows"))?;
    let mut frame = vec![0_u8; frame_len];
    for row_index in 0..height {
        let src_start = row_index
            .checked_mul(padded_bytes_per_row)
            .ok_or_else(|| anyhow!("source row offset overflow during mapped row copy"))?;
        let src_end = src_start
            .checked_add(unpadded_bytes_per_row)
            .ok_or_else(|| anyhow!("source row end overflow during mapped row copy"))?;
        let dst_start = row_index
            .checked_mul(unpadded_bytes_per_row)
            .ok_or_else(|| anyhow!("destination row offset overflow during mapped row copy"))?;
        let dst_end = dst_start
            .checked_add(unpadded_bytes_per_row)
            .ok_or_else(|| anyhow!("destination row end overflow during mapped row copy"))?;
        frame[dst_start..dst_end].copy_from_slice(&mapped[src_start..src_end]);
    }

    Ok(frame)
}

fn load_rgba_image(image_path: &Path, layer_id: &str) -> Result<image::RgbaImage> {
    let image = ImageReader::open(image_path)
        .with_context(|| {
            format!(
                "layer '{layer_id}': failed opening {}",
                image_path.display()
            )
        })?
        .decode()
        .with_context(|| {
            format!(
                "layer '{layer_id}': failed decoding {}",
                image_path.display()
            )
        })?;
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

    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| anyhow!("failed to allocate software pixmap for '{}'", layer_id))?;
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

fn render_procedural_pixmap(
    source: &ProceduralSource,
    width: u32,
    height: u32,
    context: &ExpressionContext<'_>,
) -> Result<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| anyhow!("failed to allocate procedural pixmap {width}x{height}"))?;
    pixmap.fill(Color::TRANSPARENT);

    match source {
        ProceduralSource::SolidColor { color } => {
            pixmap.fill(color_to_skia(color.evaluate(context)?));
        }
        ProceduralSource::Gradient {
            start_color,
            end_color,
            direction,
        } => {
            let sc = start_color.evaluate(context)?;
            let ec = end_color.evaluate(context)?;
            let mut paint = Paint::default();
            let (start, end) = match direction {
                GradientDirection::Horizontal => {
                    (Point::from_xy(0.0, 0.0), Point::from_xy(width as f32, 0.0))
                }
                GradientDirection::Vertical => {
                    (Point::from_xy(0.0, 0.0), Point::from_xy(0.0, height as f32))
                }
            };
            paint.shader = LinearGradient::new(
                start,
                end,
                vec![
                    GradientStop::new(0.0, color_to_skia(sc)),
                    GradientStop::new(1.0, color_to_skia(ec)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
            .ok_or_else(|| anyhow!("failed to create gradient"))?;
            let fill_rect = Rect::from_xywh(0.0, 0.0, width as f32, height as f32)
                .ok_or_else(|| anyhow!("failed to build gradient fill rect {width}x{height}"))?;
            pixmap.fill_rect(fill_rect, &paint, Transform::identity(), None);
        }
        ProceduralSource::Triangle { p0, p1, p2, color } => {
            let c = color.evaluate(context)?;
            let mut path = PathBuilder::new();
            path.move_to(p0.x * width as f32, p0.y * height as f32);
            path.line_to(p1.x * width as f32, p1.y * height as f32);
            path.line_to(p2.x * width as f32, p2.y * height as f32);
            path.close();

            let path = path
                .finish()
                .ok_or_else(|| anyhow!("failed to create triangle path"))?;
            let mut paint = Paint::default();
            paint.set_color(color_to_skia(c));
            paint.anti_alias = false;

            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        ProceduralSource::Circle {
            center,
            radius,
            color,
        } => {
            let c = color.evaluate(context)?;
            let r = radius.evaluate_with_context(context)?;
            let mut path = PathBuilder::new();
            path.push_circle(
                center.x * width as f32,
                center.y * height as f32,
                r * width as f32,
            );

            let path = path
                .finish()
                .ok_or_else(|| anyhow!("failed to create circle path"))?;
            let mut paint = Paint::default();
            paint.set_color(color_to_skia(c));
            paint.anti_alias = false;

            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        ProceduralSource::RoundedRect {
            center,
            size,
            corner_radius,
            color,
        } => {
            let c = color.evaluate(context)?;
            let cr = corner_radius.evaluate_with_context(context)?;
            let w = size.x * width as f32;
            let h = size.y * height as f32;
            let cx = center.x * width as f32;
            let cy = center.y * height as f32;
            let x = cx - w * 0.5;
            let y = cy - h * 0.5;
            let r = cr * width as f32;

            let mut pb = PathBuilder::new();
            pb.move_to(x + r, y);
            pb.line_to(x + w - r, y);
            pb.quad_to(x + w, y, x + w, y + r);
            pb.line_to(x + w, y + h - r);
            pb.quad_to(x + w, y + h, x + w - r, y + h);
            pb.line_to(x + r, y + h);
            pb.quad_to(x, y + h, x, y + h - r);
            pb.line_to(x, y + r);
            pb.quad_to(x, y, x + r, y);
            pb.close();

            let path = pb
                .finish()
                .ok_or_else(|| anyhow!("failed to create rounded rect path"))?;
            let mut paint = Paint::default();
            paint.set_color(color_to_skia(c));
            paint.anti_alias = false;
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        ProceduralSource::Ring {
            center,
            outer_radius,
            inner_radius,
            color,
        } => {
            let c = color.evaluate(context)?;
            let cx = center.x * width as f32;
            let cy = center.y * height as f32;
            let or = outer_radius.evaluate_with_context(context)? * width as f32;
            let ir = inner_radius.evaluate_with_context(context)? * width as f32;

            let mut pb = PathBuilder::new();
            pb.push_circle(cx, cy, or);
            pb.push_circle(cx, cy, ir);

            let path = pb
                .finish()
                .ok_or_else(|| anyhow!("failed to create ring path"))?;
            let mut paint = Paint::default();
            paint.set_color(color_to_skia(c));
            paint.anti_alias = false;
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::EvenOdd,
                Transform::identity(),
                None,
            );
        }
        ProceduralSource::Line {
            start,
            end,
            thickness,
            color,
        } => {
            let c = color.evaluate(context)?;
            let t = thickness.evaluate_with_context(context)?;
            let sx = start.x * width as f32;
            let sy = start.y * height as f32;
            let ex = end.x * width as f32;
            let ey = end.y * height as f32;
            let half_t = t * width as f32 * 0.5;

            let dx = ex - sx;
            let dy = ey - sy;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                // Degenerate line, skip
            } else {
                let nx = -dy / len * half_t;
                let ny = dx / len * half_t;
                let mut pb = PathBuilder::new();
                pb.move_to(sx + nx, sy + ny);
                pb.line_to(ex + nx, ey + ny);
                pb.line_to(ex - nx, ey - ny);
                pb.line_to(sx - nx, sy - ny);
                pb.close();
                let path = pb
                    .finish()
                    .ok_or_else(|| anyhow!("failed to create line path"))?;
                let mut paint = Paint::default();
                paint.set_color(color_to_skia(c));
                paint.anti_alias = false;
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
        ProceduralSource::Polygon {
            center,
            radius,
            sides,
            color,
        } => {
            let c = color.evaluate(context)?;
            let r_val = radius.evaluate_with_context(context)?;
            let cx = center.x * width as f32;
            let cy = center.y * height as f32;
            let r = r_val * width as f32;
            let n = *sides;

            let mut pb = PathBuilder::new();
            for i in 0..n {
                let angle = 2.0 * std::f32::consts::PI * (i as f32) / (n as f32)
                    - std::f32::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    pb.move_to(px, py);
                } else {
                    pb.line_to(px, py);
                }
            }
            pb.close();
            let path = pb
                .finish()
                .ok_or_else(|| anyhow!("failed to create polygon path"))?;
            let mut paint = Paint::default();
            paint.set_color(color_to_skia(c));
            paint.anti_alias = false;
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
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
    let font_file = match layer.text.font_family.to_lowercase().as_str() {
        "geistpixel-line" | "line" => "GeistPixel-Line.ttf",
        "geistpixel-square" | "square" => "GeistPixel-Square.ttf",
        "geistpixel-grid" | "grid" => "GeistPixel-Grid.ttf",
        "geistpixel-circle" | "circle" => "GeistPixel-Circle.ttf",
        "geistpixel-triangle" | "triangle" => "GeistPixel-Triangle.ttf",
        _ => "GeistPixel-Line.ttf",
    };

    let repo_font_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/fonts/geist_pixel")
        .join(font_file);
    let home_font_path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Fonts").join(font_file));
    let font_path = if repo_font_path.exists() {
        repo_font_path
    } else if let Some(home_path) = home_font_path {
        home_path
    } else {
        repo_font_path
    };

    let font_data = std::fs::read(&font_path)
        .with_context(|| format!("failed to read font file {}", font_path.display()))?;
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

    layout.append(
        &[&font],
        &fontdue::layout::TextStyle::new(&layer.text.content, layer.text.font_size, 0),
    );

    let glyphs = layout.glyphs();
    if glyphs.is_empty() {
        return Pixmap::new(1, 1)
            .ok_or_else(|| anyhow!("failed to allocate fallback pixmap for empty text layer"));
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
                if alpha == 0 {
                    continue;
                }

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
                            let out_g =
                                (pg as f32 + pixel.green() as f32 * (1.0 - src_a)).min(255.0);
                            let out_b =
                                (pb as f32 + pixel.blue() as f32 * (1.0 - src_a)).min(255.0);
                            *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                                out_r as u8,
                                out_g as u8,
                                out_b as u8,
                                (out_a * 255.0).round() as u8,
                            )
                            .ok_or_else(|| {
                                anyhow!(
                                    "invalid premultiplied color while rasterizing text layer '{}'",
                                    layer.common.id
                                )
                            })?;
                        }
                    }
                }
            }
        }
    }

    Ok(pixmap)
}

#[allow(clippy::too_many_arguments)]
fn build_shader_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    layer: &ShaderLayer,
    group_chain: Vec<Group>,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64,
    fps: u32,
) -> Result<GpuLayer> {
    // Load shader source
    let user_fragment = if let Some(fragment) = &layer.shader.fragment {
        fragment.clone()
    } else if let Some(path) = &layer.shader.path {
        std::fs::read_to_string(path).with_context(|| {
            format!(
                "layer '{}': failed reading shader file {}",
                layer.common.id,
                path.display()
            )
        })?
    } else {
        bail!(
            "layer '{}': shader must have fragment or path",
            layer.common.id
        );
    };

    let full_wgsl = format!("{}\n{}", user_fragment, CUSTOM_SHADER_PREAMBLE);

    // Create per-layer pipeline
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&format!("vcr-custom-shader-{}", layer.common.id)),
        source: wgpu::ShaderSource::Wgsl(full_wgsl.into()),
    });

    let shader_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("vcr-shader-bgl-{}", layer.common.id)),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<ShaderUniform>() as u64
                    ),
                },
                count: None,
            }],
        });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("vcr-shader-pl-{}", layer.common.id)),
        bind_group_layouts: &[&shader_bind_group_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&format!("vcr-shader-pipeline-{}", layer.common.id)),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader_module,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader_module,
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

    // Create offscreen texture
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("vcr-shader-tex-{}", layer.common.id)),
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

    // Create uniform buffer
    let init_context = ExpressionContext::new(0.0, params, seed);
    let mut custom = [0.0_f32; 8];
    let uniform_props: Vec<ScalarProperty> = layer.shader.uniforms.values().cloned().collect();
    for (i, prop) in uniform_props.iter().enumerate() {
        custom[i] = prop.evaluate_with_context(&init_context)?;
    }
    let initial_uniform = ShaderUniform {
        time: 0.0,
        frame: 0,
        resolution: [frame_width as f32, frame_height as f32],
        custom,
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("vcr-shader-uniform-{}", layer.common.id)),
        contents: bytemuck::bytes_of(&initial_uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("vcr-shader-bg-{}", layer.common.id)),
        layout: &shader_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let is_static = uniform_props.iter().all(ScalarProperty::is_static);

    // Evaluate initial layer state
    let state = evaluate_layer_state_or_hidden(
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
    )?;

    let blend_texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let draw_resources = build_layer_draw_resources(
        device,
        queue,
        frame_width,
        frame_height,
        &layer.common,
        state.position,
        state.scale,
        state.rotation_degrees,
        state.opacity,
        frame_width,
        frame_height,
        &blend_texture_view,
        blend_bind_group_layout,
        sampler,
    )?;

    let shader_gpu = CustomShaderGpu {
        uniforms: uniform_props,
        uniform_buffer,
        bind_group,
        pipeline,
        _texture: texture,
        view,
        has_rendered: false,
        is_static,
        last_rendered_frame: None,
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
            && group_chain.iter().all(Group::has_static_properties)
            && is_static,
        group_chain,
        anchor: layer.common.anchor,
        uniform_buffer: draw_resources.uniform_buffer,
        blend_bind_group: draw_resources.blend_bind_group,
        vertex_buffer: draw_resources.vertex_buffer,
        last_vertices: Some(draw_resources.initial_vertices),
        last_opacity: Some(draw_resources.initial_opacity),
        source: GpuLayerSource::Shader(shader_gpu),
    })
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
            bytes_per_row: Some(
                checked_bytes_per_row(
                    layer_width,
                    &format!("text layer '{}' width", layer.common.id),
                )?
                .get(),
            ),
            rows_per_image: Some(layer_height),
        },
        wgpu::Extent3d {
            width: layer_width,
            height: layer_height,
            depth_or_array_layers: 1,
        },
    );

    let state = evaluate_layer_state_or_hidden(
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
    )?;

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
        state.opacity,
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
        group_chain,
        anchor: layer.common.anchor,
        uniform_buffer: draw_resources.uniform_buffer,
        blend_bind_group: draw_resources.blend_bind_group,
        vertex_buffer: draw_resources.vertex_buffer,
        last_vertices: Some(draw_resources.initial_vertices),
        last_opacity: Some(draw_resources.initial_opacity),
        source: GpuLayerSource::Text { _texture: texture },
    })
}

#[allow(clippy::too_many_arguments)]
fn build_ascii_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame_width: u32,
    frame_height: u32,
    layer: &AsciiLayer,
    group_chain: Vec<Group>,
    blend_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    params: &Parameters,
    modulators: &ModulatorMap,
    seed: u64,
    fps: u32,
) -> Result<GpuLayer> {
    let prepared = PreparedAsciiLayer::new(&layer.ascii, &layer.common.id)?;
    let layer_width = prepared.pixel_width();
    let layer_height = prepared.pixel_height();
    let is_static = prepared.is_static();
    let initial_pixmap = prepared.render_frame_pixmap(0)?;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("vcr-ascii-layer-{}", layer.common.id)),
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
    queue_write_pixmap_texture(
        queue,
        &texture,
        initial_pixmap.as_ref(),
        &format!("ascii layer '{}'", layer.common.id),
    )?;

    let state = evaluate_layer_state_or_hidden(
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
    )?;

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
        state.opacity,
        layer_width,
        layer_height,
        &texture_view,
        blend_bind_group_layout,
        sampler,
    )?;

    let ascii_gpu = AsciiGpu {
        prepared,
        texture,
        has_rendered: true,
        is_static,
        last_rendered_frame: Some(0),
    };

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
            && group_chain.iter().all(Group::has_static_properties)
            && is_static,
        group_chain,
        anchor: layer.common.anchor,
        uniform_buffer: draw_resources.uniform_buffer,
        blend_bind_group: draw_resources.blend_bind_group,
        vertex_buffer: draw_resources.vertex_buffer,
        last_vertices: Some(draw_resources.initial_vertices),
        last_opacity: Some(draw_resources.initial_opacity),
        source: GpuLayerSource::Ascii(ascii_gpu),
    })
}

fn queue_write_pixmap_texture(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    pixmap: tiny_skia::PixmapRef<'_>,
    label: &str,
) -> Result<()> {
    let bytes_per_row = checked_bytes_per_row(pixmap.width(), label)?.get();
    let rows_per_image = NonZeroU32::new(pixmap.height())
        .ok_or_else(|| anyhow!("{label} has invalid height {}", pixmap.height()))?
        .get();

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixmap.data(),
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(rows_per_image),
        },
        wgpu::Extent3d {
            width: pixmap.width(),
            height: pixmap.height(),
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        align_to, can_use_software_fallback, checked_bytes_per_row, copy_tight_rows,
        shader_layer_needs_update, SoftwareRenderer, PROCEDURAL_SHADER,
    };
    use crate::schema::Manifest;
    use crate::timeline::RenderSceneData;

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
    fn copy_tight_rows_rejects_unpadded_rows_larger_than_padded_rows() {
        let mapped = vec![0_u8; 8];
        let error = copy_tight_rows(&mapped, 8, 4, 1)
            .expect_err("unpadded row larger than padded row should fail");
        assert!(error.to_string().contains("cannot exceed"));
    }

    #[test]
    fn align_to_returns_error_on_overflow() {
        let error = align_to(u32::MAX, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            .expect_err("alignment overflow should return error");
        assert!(error.to_string().contains("overflow"));
    }

    #[test]
    fn checked_bytes_per_row_rejects_width_overflow() {
        let error = checked_bytes_per_row(u32::MAX, "test width")
            .expect_err("bytes_per_row overflow should return error");
        assert!(error.to_string().contains("overflows"));
    }

    #[test]
    fn shader_layer_needs_update_when_frame_advances() {
        assert!(shader_layer_needs_update(None, 0));
        assert!(!shader_layer_needs_update(Some(0), 0));
        assert!(shader_layer_needs_update(Some(0), 1));
    }

    #[test]
    fn software_fallback_is_rejected_for_no_gpu_error_when_manifest_has_shader_layers() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: shader-only
    shader:
      fragment: |
        fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
          return vec4<f32>(uv.x, uv.y, 0.0, 1.0);
        }
"#,
        )
        .expect("manifest should parse");

        assert!(!can_use_software_fallback(
            "failed: no suitable GPU adapter found",
            &manifest.layers
        ));
    }

    #[test]
    fn procedural_shader_triangle_path_returns_color_for_interior_pixels() {
        assert!(PROCEDURAL_SHADER
            .contains("if !(has_neg && has_pos) {\n      return procedural.color_a;"));
        assert!(PROCEDURAL_SHADER.contains("return transparent;\n  }\n\n  // 3: Circle"));
    }

    #[test]
    fn procedural_shader_default_fallback_is_transparent() {
        assert!(PROCEDURAL_SHADER.contains("// Default fallback\n  return transparent;"));
        assert!(!PROCEDURAL_SHADER.contains("vec4<f32>(0.0, 0.0, 0.0, 1.0)"));
    }

    #[test]
    fn software_triangle_fill_has_opaque_interior_and_transparent_exterior() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: tri
    procedural:
      kind: triangle
      p0: [0.1, 0.1]
      p1: [0.9, 0.1]
      p2: [0.5, 0.9]
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
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
            .render_frame_rgba(0)
            .expect("triangle frame render should succeed");
        assert!(
            pixel_at(&frame, 8, 4, 4)[3] > 0,
            "triangle interior should be opaque"
        );
        assert_eq!(
            pixel_at(&frame, 8, 0, 7)[3],
            0,
            "triangle exterior should stay transparent"
        );
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

    fn pixel_at(frame: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let offset = (y * width + x) * 4;
        [
            frame[offset],
            frame[offset + 1],
            frame[offset + 2],
            frame[offset + 3],
        ]
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
