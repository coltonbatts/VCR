use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use tiny_skia::Pixmap;
use wgpu::util::DeviceExt;

use crate::ascii::PreparedAsciiLayer;
use crate::color;
use crate::schema::{
    Anchor, AnimatableColor, AsciiLayer, AssetLayer, ColorSpace, Environment, ExpressionContext,
    GradientDirection, Group, ImageLayer, Layer, LayerCommon, ModulatorBinding, ModulatorMap,
    Parameters, ProceduralLayer, ProceduralSource, PropertyValue, ScalarProperty, ShaderLayer,
    TextLayer, TimeBase, TimingControls, Vec2,
};
use crate::timeline::{
    evaluate_layer_state, evaluate_layer_state_or_hidden, layer_source_frame, resolve_group_chain,
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

// Layer textures hold premultiplied linear color (sRGB-encoded storage, decoded by the
// sampler before filtering), so opacity scales every channel and the pipeline blends with
// premultiplied source-over into a float accumulator.
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  return textureSample(layer_tex, layer_sampler, input.uv) * layer.opacity;
}
"#;

// Must stay formula-for-formula identical to `procedural_premultiplied` (software backend).
// Colors arrive premultiplied in linear light. Geometry is evaluated in pixel space: centers,
// points and sizes are fractions of the frame's width/height; radii and thickness are
// fractions of the frame width, so circles stay circular at any aspect ratio.
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
  resolution: vec2<f32>,
  _padding2: vec2<f32>,
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
  // Recompute uv from the pixel center so both backends sample identical positions.
  let res = procedural.resolution;
  let uv = clamp(input.position.xy / res, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
  let px = uv * res;
  let unit = res.x;
  let transparent = vec4<f32>(0.0, 0.0, 0.0, 0.0);

  // 0: SolidColor
  if procedural.kind == 0u {
    return procedural.color_a;
  }

  // 1: Gradient (interpolates premultiplied linear endpoints)
  if procedural.kind == 1u {
    let amount = select(uv.y, uv.x, procedural.axis == 0u);
    return procedural.color_a + (procedural.color_b - procedural.color_a) * amount;
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
    let dist = distance(px, procedural.p0 * res);
    if dist < procedural.radius * unit {
      return procedural.color_a;
    }
    return transparent;
  }

  // 4: RoundedRect
  if procedural.kind == 4u {
    let half_size = procedural.size * res * 0.5;
    let r = procedural.corner_radius * unit;
    let d = abs(px - procedural.p0 * res) - half_size + vec2<f32>(r);
    let sdf = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - r;
    if sdf <= 0.0 {
      return procedural.color_a;
    }
    return transparent;
  }

  // 5: Ring
  if procedural.kind == 5u {
    let dist = distance(px, procedural.p0 * res);
    if dist <= procedural.radius * unit && dist >= procedural.inner_radius * unit {
      return procedural.color_a;
    }
    return transparent;
  }

  // 6: Line (capsule SDF)
  if procedural.kind == 6u {
    let a = procedural.p0 * res;
    let ab = procedural.p1 * res - a;
    let ap = px - a;
    let len_sq = dot(ab, ab);
    var t_line = 0.0;
    if len_sq > 0.000001 {
      t_line = clamp(dot(ap, ab) / len_sq, 0.0, 1.0);
    }
    let dist = distance(px, a + ab * t_line);
    if dist <= procedural.thickness * unit * 0.5 {
      return procedural.color_a;
    }
    return transparent;
  }

  // 7: Polygon (regular n-gon, first vertex pointing up)
  if procedural.kind == 7u {
    let n = f32(procedural.extra_u32);
    let p = px - procedural.p0 * res;
    let angle = atan2(p.y, p.x) + PI * 0.5;
    let sector = 2.0 * PI / n;
    let r = length(p);
    let theta = ((angle % sector) + sector) % sector;
    let half_sector = sector * 0.5;
    let edge_dist = procedural.radius * unit * cos(half_sector);
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
  custom: array<f32, 8>,
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

fn vcr_srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
  let low = c / 12.92;
  let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
  return select(high, low, c <= vec3<f32>(0.04045));
}

// `shade` returns straight-alpha sRGB-encoded color (like manifest colors); convert it to the
// pipeline's premultiplied linear representation.
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let c = clamp(shade(input.uv, vcr_uniforms), vec4<f32>(0.0), vec4<f32>(1.0));
  return vec4<f32>(vcr_srgb_to_linear(c.rgb) * c.a, c.a);
}
"#;

// Converts the float accumulator (premultiplied linear) to the output target: unpremultiply,
// then sRGB-encode (in shader, or by hardware when the target format is *Srgb).
// Must match `color::encode_output`.
const RESOLVE_SHADER: &str = r#"
struct ResolveUniform {
  encode_srgb: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
}

@group(0) @binding(0) var accum_tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> resolve: ResolveUniform;

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

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
  let low = c * 12.92;
  let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
  return select(high, low, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
  let dims = textureDimensions(accum_tex);
  let coord = min(vec2<u32>(floor(input.uv * vec2<f32>(dims))), dims - vec2<u32>(1u, 1u));
  let premultiplied = textureLoad(accum_tex, coord, 0);
  let alpha = clamp(premultiplied.a, 0.0, 1.0);
  if alpha < 0.5 / 255.0 {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }
  var rgb = clamp(premultiplied.rgb / alpha, vec3<f32>(0.0), vec3<f32>(1.0));
  if resolve.encode_srgb == 1u {
    rgb = linear_to_srgb(rgb);
  }
  return vec4<f32>(rgb, alpha);
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
    resolution: [f32; 2],
    _padding2: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ResolveUniform {
    encode_srgb: u32,
    _pad: [u32; 3],
}

/// Float accumulator for layer compositing (premultiplied linear light).
const ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Storage for every layer texture: premultiplied linear, sRGB-encoded 8-bit.
const LAYER_TEXEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

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
    time_base: TimeBase,
    seed: u64,
    params: Parameters,
    modulators: ModulatorMap,
    output_texture: wgpu::Texture,
    _accum_texture: wgpu::Texture,
    accum_view: wgpu::TextureView,
    resolve_pipeline: wgpu::RenderPipeline,
    resolve_bind_group: wgpu::BindGroup,
    _resolve_uniform_buffer: wgpu::Buffer,
    readback_buffers: [wgpu::Buffer; READBACK_BUFFER_COUNT],
    next_readback_index: usize,
    pending_readback: Option<PendingReadback>,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    blend_pipeline: wgpu::RenderPipeline,
    procedural_pipeline: wgpu::RenderPipeline,
    layers: Vec<GpuLayer>,
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
    /// Places where this backend cannot render the manifest faithfully (recorded in metadata).
    warnings: Vec<String>,
}

enum RendererBackend {
    Gpu(GpuRenderer),
    Software(SoftwareRenderer),
}

struct SoftwareRenderer {
    width: u32,
    height: u32,
    time_base: TimeBase,
    seed: u64,
    params: Parameters,
    modulators: ModulatorMap,
    layers: Vec<SoftwareLayer>,
    warnings: Vec<String>,
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
    Asset {
        texels: Texels,
    },
    Procedural {
        source: ProceduralSource,
        is_static: bool,
        cached: Option<Texels>,
    },
    Shader,
    Text {
        texels: Texels,
    },
    Ascii {
        prepared: PreparedAsciiLayer,
    },
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
        let time_base = scene.time_base(environment.fps);
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let accum_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vcr-accumulation-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ACCUM_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let accum_view = accum_texture.create_view(&wgpu::TextureViewDescriptor::default());

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
                    format: ACCUM_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
                    format: LAYER_TEXEL_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
        });

        let resolve_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vcr-resolve-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<
                                ResolveUniform,
                            >()
                                as u64),
                        },
                        count: None,
                    },
                ],
            });
        let resolve_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vcr-resolve-uniform"),
            contents: bytemuck::bytes_of(&ResolveUniform {
                // *Srgb targets encode in hardware; everything else is encoded in the shader.
                encode_srgb: u32::from(!render_format.is_srgb()),
                _pad: [0; 3],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let resolve_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vcr-resolve-bind-group"),
            layout: &resolve_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&accum_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: resolve_uniform_buffer.as_entire_binding(),
                },
            ],
        });
        let resolve_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vcr-resolve-shader"),
            source: wgpu::ShaderSource::Wgsl(RESOLVE_SHADER.into()),
        });
        let resolve_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("vcr-resolve-pipeline-layout"),
                bind_group_layouts: &[&resolve_bind_group_layout],
                push_constant_ranges: &[],
            });
        let resolve_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vcr-resolve-pipeline"),
            layout: Some(&resolve_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &resolve_shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &resolve_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_format,
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
                    time_base,
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
                    time_base,
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
                    time_base,
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
                    time_base,
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
                    time_base,
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
                    time_base,
                )?,
            };
            gpu_layers.push(gpu_layer);
        }
        gpu_layers.sort_by_key(|layer| layer.z_index);

        Ok(Self {
            adapter_name: context.adapter_name.clone(),
            adapter_backend: context.adapter_backend,
            device,
            queue,
            width,
            height,
            time_base,
            seed: scene.seed,
            params: scene.params.clone(),
            modulators: scene.modulators.clone(),
            output_texture,
            _accum_texture: accum_texture,
            accum_view,
            resolve_pipeline,
            resolve_bind_group,
            _resolve_uniform_buffer: resolve_uniform_buffer,
            readback_buffers,
            next_readback_index: 0,
            pending_readback: None,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            blend_pipeline,
            procedural_pipeline,
            layers: gpu_layers,
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
                view: &self.accum_view,
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
                    self.time_base,
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
        drop(render_pass);

        let mut resolve_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vcr-resolve-pass"),
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
        resolve_pass.set_pipeline(&self.resolve_pipeline);
        resolve_pass.set_bind_group(0, &self.resolve_bind_group, &[]);
        resolve_pass.draw(0..3, 0..1);

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
        for layer in &mut self.layers {
            let Some(source_frame) = layer_source_frame(
                layer.timing,
                &layer.group_chain,
                frame_index,
                self.time_base,
            ) else {
                continue;
            };
            let context =
                ExpressionContext::new(source_frame, self.time_base, &self.params, self.seed);
            let GpuLayerSource::Procedural(procedural) = &mut layer.source else {
                continue;
            };

            let needs_update = !procedural.has_rendered || !procedural.is_static;
            if !needs_update {
                continue;
            }

            let uniform =
                evaluate_procedural_uniform(&procedural.source, &context, self.width, self.height)?;
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
        for layer in &mut self.layers {
            let Some(source_frame) = layer_source_frame(
                layer.timing,
                &layer.group_chain,
                frame_index,
                self.time_base,
            ) else {
                continue;
            };
            let context =
                ExpressionContext::new(source_frame, self.time_base, &self.params, self.seed);
            let GpuLayerSource::Shader(shader) = &mut layer.source else {
                continue;
            };

            let needs_update = shader_layer_needs_update(shader.last_rendered_frame, frame_index);
            if !needs_update {
                continue;
            }

            let time = context.seconds();
            let mut custom = [0.0_f32; 8];
            for (i, prop) in shader.uniforms.iter().enumerate() {
                custom[i] = prop.evaluate_with_context(&context)?;
            }
            let uniform = ShaderUniform {
                time,
                frame: source_frame.max(0.0).floor() as u32,
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
            let Some(source_frame) = layer_source_frame(
                layer.timing,
                &layer.group_chain,
                frame_index,
                self.time_base,
            ) else {
                continue;
            };
            let GpuLayerSource::Ascii(ascii) = &mut layer.source else {
                continue;
            };

            let needs_update = !ascii.has_rendered || !ascii.is_static;
            if !needs_update {
                continue;
            }

            let pixmap = ascii
                .prepared
                .render_frame_pixmap(source_frame.max(0.0).floor() as u32)?;
            queue_write_texels(
                &self.queue,
                &ascii.texture,
                &Texels::from_srgb_premultiplied_pixmap(&pixmap),
                &format!("ascii layer '{}'", layer.id),
            )?;
            ascii.has_rendered = true;
            ascii.last_rendered_frame = Some(frame_index);
        }

        Ok(())
    }
}

impl Renderer {
    fn from_software(software: SoftwareRenderer, backend_reason: String) -> Self {
        Self {
            warnings: software.warnings.clone(),
            backend: RendererBackend::Software(software),
            backend_reason,
        }
    }

    fn from_gpu(gpu: GpuRenderer) -> Self {
        Self {
            backend_reason: format!("adapter '{}' ({:?})", gpu.adapter_name, gpu.adapter_backend),
            backend: RendererBackend::Gpu(gpu),
            warnings: Vec::new(),
        }
    }

    fn with_environment_warnings(mut self, environment: &Environment) -> Self {
        if environment.color_space != ColorSpace::Rec709 {
            let warning = format!(
                "environment.color_space {:?} is not implemented: frames are rendered in sRGB/BT.709 primaries and encodes are tagged BT.709",
                environment.color_space
            );
            eprintln!("[VCR] WARNING: {warning}");
            self.warnings.push(warning);
        }
        self
    }

    /// Features this backend could not render faithfully. Callers must surface these and
    /// record them in render metadata.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn new_software(
        environment: &Environment,
        layers: &[Layer],
        scene: RenderSceneData,
    ) -> Result<Self> {
        let software = SoftwareRenderer::new(environment, layers, &scene)
            .context("failed to initialize software renderer")?;
        Ok(
            Self::from_software(software, "forced software backend".to_owned())
                .with_environment_warnings(environment),
        )
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
                    return Ok(Self::from_software(software, error_message)
                        .with_environment_warnings(environment));
                }
                if error_message.contains(NO_GPU_ADAPTER_ERR) && has_shader_layers(layers) {
                    return Err(error.context(
                        "software fallback is disabled because the manifest contains shader layers",
                    ));
                }
                return Err(error);
            }
        };

        Ok(Self::from_gpu(gpu).with_environment_warnings(environment))
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
                    return Ok(Self::from_software(software, error_message)
                        .with_environment_warnings(environment));
                }
                return Err(error);
            }
        };

        Ok(Self::from_gpu(gpu).with_environment_warnings(environment))
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
        let time_base = scene.time_base(environment.fps);
        let width = environment.resolution.width;
        let height = environment.resolution.height;
        let groups_by_id = resolve_groups_by_id(&scene.groups);
        let mut software_layers = Vec::with_capacity(layers.len());
        let mut warnings = Vec::new();

        for layer in layers {
            let common = layer.common();
            let group_chain = resolve_group_chain(common, &groups_by_id)?;
            let source = match layer {
                Layer::Asset(asset_layer) => SoftwareLayerSource::Asset {
                    texels: load_image_texels(&asset_layer.source_path, &asset_layer.common.id)?,
                },
                Layer::Image(image_layer) => SoftwareLayerSource::Asset {
                    texels: load_image_texels(&image_layer.image.path, &image_layer.common.id)?,
                },
                Layer::Procedural(procedural_layer) => SoftwareLayerSource::Procedural {
                    source: procedural_layer.procedural.clone(),
                    is_static: procedural_layer.procedural.is_static(),
                    cached: None,
                },
                Layer::Shader(shader_layer) => {
                    let warning = format!(
                        "layer '{}': custom WGSL shader layers cannot run on the software backend and are rendered as fully transparent",
                        shader_layer.common.id
                    );
                    eprintln!("[VCR] WARNING: {warning}");
                    warnings.push(warning);
                    SoftwareLayerSource::Shader
                }
                Layer::Text(text_layer) => SoftwareLayerSource::Text {
                    texels: render_text_texels(text_layer)?,
                },
                Layer::Ascii(ascii_layer) => SoftwareLayerSource::Ascii {
                    prepared: PreparedAsciiLayer::new(&ascii_layer.ascii, &ascii_layer.common.id)?,
                },
            };

            let (layer_width, layer_height) = match &source {
                SoftwareLayerSource::Asset { texels } | SoftwareLayerSource::Text { texels } => {
                    (texels.width, texels.height)
                }
                SoftwareLayerSource::Procedural { .. } | SoftwareLayerSource::Shader => {
                    (width, height)
                }
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
            time_base,
            seed: scene.seed,
            params: scene.params.clone(),
            modulators: scene.modulators.clone(),
            layers: software_layers,
            warnings,
        })
    }

    fn render_frame_rgba(&mut self, frame_index: u32) -> Result<Vec<u8>> {
        let mut accum = vec![[0.0_f32; 4]; (self.width * self.height) as usize];
        for index in 0..self.layers.len() {
            self.render_layer(&mut accum, index, frame_index)?;
        }

        let mut frame = Vec::with_capacity(accum.len() * 4);
        for pixel in accum {
            frame.extend_from_slice(&color::encode_output(pixel));
        }
        Ok(frame)
    }

    fn render_layer(
        &mut self,
        accum: &mut [[f32; 4]],
        layer_index: usize,
        frame_index: u32,
    ) -> Result<()> {
        let (width, height, time_base, seed) = (self.width, self.height, self.time_base, self.seed);
        let layer = &mut self.layers[layer_index];
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
            time_base,
            &self.params,
            seed,
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
        let Some(source_frame) =
            layer_source_frame(layer.timing, &layer.group_chain, frame_index, time_base)
        else {
            return Ok(());
        };

        match &mut layer.source {
            SoftwareLayerSource::Asset { texels } | SoftwareLayerSource::Text { texels } => {
                composite_texels(accum, width, height, texels, opacity, transform);
            }
            SoftwareLayerSource::Procedural {
                source,
                is_static,
                cached,
            } => {
                if cached.is_none() || !*is_static {
                    let context =
                        ExpressionContext::new(source_frame, time_base, &self.params, seed);
                    let uniform = evaluate_procedural_uniform(source, &context, width, height)?;
                    *cached = Some(render_procedural_texels(&uniform, width, height));
                }
                if let Some(texels) = cached.as_ref() {
                    composite_texels(accum, width, height, texels, opacity, transform);
                }
            }
            SoftwareLayerSource::Shader => {
                // Recorded as a warning at construction; see `Renderer::warnings`.
            }
            SoftwareLayerSource::Ascii { prepared } => {
                let pixmap = prepared.render_frame_pixmap(source_frame.max(0.0).floor() as u32)?;
                let texels = Texels::from_srgb_premultiplied_pixmap(&pixmap);
                composite_texels(accum, width, height, &texels, opacity, transform);
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
    time_base: TimeBase,
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
        time_base,
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
    time_base: TimeBase,
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
        time_base,
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
    time_base: TimeBase,
) -> Result<GpuLayer> {
    let texels = load_image_texels(image_path, &common.id)?;
    let (layer_width, layer_height) = (texels.width, texels.height);

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
        format: LAYER_TEXEL_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue_write_texels(queue, &texture, &texels, &format!("layer '{}'", common.id))?;

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
        time_base,
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
    time_base: TimeBase,
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
        format: LAYER_TEXEL_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let init_context = ExpressionContext::new(0.0, time_base, params, seed);
    let uniform =
        evaluate_procedural_uniform(&layer.procedural, &init_context, frame_width, frame_height)?;
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
        time_base,
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
    time_base: TimeBase,
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
        time_base,
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

/// Shared by both backends: the GPU uploads it, the software backend evaluates it per pixel
/// with `procedural_premultiplied`. Colors are converted to premultiplied linear here.
fn evaluate_procedural_uniform(
    source: &ProceduralSource,
    context: &ExpressionContext<'_>,
    width: u32,
    height: u32,
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
        resolution: [width as f32, height as f32],
        _padding2: [0.0; 2],
    };

    fn eval_color(c: &AnimatableColor, ctx: &ExpressionContext<'_>) -> Result<[f32; 4]> {
        Ok(color::premultiplied_linear(c.evaluate(ctx)?))
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

/// Software twin of `PROCEDURAL_SHADER::fs_main`, evaluated at the center of pixel (x, y).
/// Returns premultiplied linear color. Keep the two in lockstep.
fn procedural_premultiplied(uniform: &ProceduralUniform, x: u32, y: u32) -> [f32; 4] {
    const TRANSPARENT: [f32; 4] = [0.0; 4];
    let res = uniform.resolution;
    let uv = [
        ((x as f32 + 0.5) / res[0]).clamp(0.0, 1.0),
        ((y as f32 + 0.5) / res[1]).clamp(0.0, 1.0),
    ];
    let px = [uv[0] * res[0], uv[1] * res[1]];
    let unit = res[0];
    let to_px = |p: [f32; 2]| [p[0] * res[0], p[1] * res[1]];
    let distance =
        |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();

    match uniform.kind {
        0 => uniform.color_a,
        1 => {
            let amount = if uniform.axis == 0 { uv[0] } else { uv[1] };
            let (a, b) = (uniform.color_a, uniform.color_b);
            [
                a[0] + (b[0] - a[0]) * amount,
                a[1] + (b[1] - a[1]) * amount,
                a[2] + (b[2] - a[2]) * amount,
                a[3] + (b[3] - a[3]) * amount,
            ]
        }
        2 => {
            let sign = |p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]| {
                (p1[0] - p3[0]) * (p2[1] - p3[1]) - (p2[0] - p3[0]) * (p1[1] - p3[1])
            };
            let d1 = sign(uv, uniform.p0, uniform.p1);
            let d2 = sign(uv, uniform.p1, uniform.p2);
            let d3 = sign(uv, uniform.p2, uniform.p0);
            let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            if !(has_neg && has_pos) {
                uniform.color_a
            } else {
                TRANSPARENT
            }
        }
        3 => {
            if distance(px, to_px(uniform.p0)) < uniform.radius * unit {
                uniform.color_a
            } else {
                TRANSPARENT
            }
        }
        4 => {
            let center = to_px(uniform.p0);
            let half = [
                uniform.size[0] * res[0] * 0.5,
                uniform.size[1] * res[1] * 0.5,
            ];
            let r = uniform.corner_radius * unit;
            let d = [
                (px[0] - center[0]).abs() - half[0] + r,
                (px[1] - center[1]).abs() - half[1] + r,
            ];
            let outside = (d[0].max(0.0).powi(2) + d[1].max(0.0).powi(2)).sqrt();
            let sdf = outside + d[0].max(d[1]).min(0.0) - r;
            if sdf <= 0.0 {
                uniform.color_a
            } else {
                TRANSPARENT
            }
        }
        5 => {
            let dist = distance(px, to_px(uniform.p0));
            if dist <= uniform.radius * unit && dist >= uniform.inner_radius * unit {
                uniform.color_a
            } else {
                TRANSPARENT
            }
        }
        6 => {
            let a = to_px(uniform.p0);
            let b = to_px(uniform.p1);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let ap = [px[0] - a[0], px[1] - a[1]];
            let len_sq = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if len_sq > 0.000_001 {
                ((ap[0] * ab[0] + ap[1] * ab[1]) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let closest = [a[0] + ab[0] * t, a[1] + ab[1] * t];
            if distance(px, closest) <= uniform.thickness * unit * 0.5 {
                uniform.color_a
            } else {
                TRANSPARENT
            }
        }
        7 => {
            let n = uniform.extra_u32 as f32;
            let center = to_px(uniform.p0);
            let p = [px[0] - center[0], px[1] - center[1]];
            let angle = p[1].atan2(p[0]) + std::f32::consts::PI * 0.5;
            let sector = 2.0 * std::f32::consts::PI / n;
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let theta = ((angle % sector) + sector) % sector;
            let half_sector = sector * 0.5;
            let edge_dist = uniform.radius * unit * half_sector.cos();
            let proj = r * (theta - half_sector).cos();
            if proj <= edge_dist {
                uniform.color_a
            } else {
                TRANSPARENT
            }
        }
        _ => TRANSPARENT,
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

/// CPU copy of a layer texture in the pipeline's texel storage format: premultiplied linear
/// light, sRGB-encoded, 8 bits per channel (what an `Rgba8UnormSrgb` texture holds).
/// Both backends build layer textures from the same `Texels`.
struct Texels {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Texels {
    fn from_straight_srgb8(width: u32, height: u32, mut data: Vec<u8>) -> Self {
        color::straight_srgb8_to_texels(&mut data);
        Self {
            width,
            height,
            data,
        }
    }

    /// tiny-skia pixmaps (ascii raster) are premultiplied in sRGB space.
    fn from_srgb_premultiplied_pixmap(pixmap: &Pixmap) -> Self {
        let mut data = pixmap.data().to_vec();
        color::srgb_premultiplied8_to_texels(&mut data);
        Self {
            width: pixmap.width(),
            height: pixmap.height(),
            data,
        }
    }

    fn premultiplied_linear(&self, x: u32, y: u32) -> [f32; 4] {
        let offset = ((y * self.width + x) * 4) as usize;
        color::decode_texel([
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
        ])
    }

    /// Bilinear sample with clamp-to-edge at texel-space coordinates (texel centers at +0.5),
    /// the same convention as a GPU linear sampler.
    fn sample_bilinear(&self, x: f32, y: f32) -> [f32; 4] {
        let sx = x - 0.5;
        let sy = y - 0.5;
        let x0 = sx.floor();
        let y0 = sy.floor();
        // Exact weights. GPUs use reduced sub-texel precision; that is the documented source of
        // small parity differences next to high-contrast texel edges under scaling.
        let fx = sx - x0;
        let fy = sy - y0;
        let max_x = self.width as i64 - 1;
        let max_y = self.height as i64 - 1;
        let xi0 = (x0 as i64).clamp(0, max_x) as u32;
        let xi1 = (x0 as i64 + 1).clamp(0, max_x) as u32;
        let yi0 = (y0 as i64).clamp(0, max_y) as u32;
        let yi1 = (y0 as i64 + 1).clamp(0, max_y) as u32;

        let top_left = self.premultiplied_linear(xi0, yi0);
        let top_right = self.premultiplied_linear(xi1, yi0);
        let bottom_left = self.premultiplied_linear(xi0, yi1);
        let bottom_right = self.premultiplied_linear(xi1, yi1);
        let mut out = [0.0_f32; 4];
        for channel in 0..4 {
            let top = top_left[channel] + (top_right[channel] - top_left[channel]) * fx;
            let bottom = bottom_left[channel] + (bottom_right[channel] - bottom_left[channel]) * fx;
            out[channel] = top + (bottom - top) * fy;
        }
        out
    }
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

/// Image files are treated as straight-alpha sRGB (PNG/JPEG/WebP default).
fn load_image_texels(image_path: &Path, layer_id: &str) -> Result<Texels> {
    let image = load_rgba_image(image_path, layer_id)?;
    let (width, height) = image.dimensions();
    Ok(Texels::from_straight_srgb8(width, height, image.into_raw()))
}

/// Maps layer-space pixels to output pixels: `x' = a*x + c*y + tx`, `y' = b*x + d*y + ty`.
/// Same geometry as the GPU quad built by `build_layer_quad`.
#[derive(Debug, Clone, Copy)]
struct LayerTransform {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    tx: f32,
    ty: f32,
}

impl LayerTransform {
    fn map(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    fn is_integer_translation(self) -> bool {
        self.a == 1.0
            && self.b == 0.0
            && self.c == 0.0
            && self.d == 1.0
            && self.tx.fract() == 0.0
            && self.ty.fract() == 0.0
    }
}

fn layer_transform(
    position: Vec2,
    scale: Vec2,
    rotation_degrees: f32,
    width: f32,
    height: f32,
    anchor: Anchor,
) -> LayerTransform {
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

    LayerTransform {
        a,
        b,
        c,
        d,
        tx: center_x - (a * half_w + c * half_h),
        ty: center_y - (b * half_w + d * half_h),
    }
}

/// Composites `texels` into the premultiplied linear accumulator with source-over, sampling
/// each covered output pixel center through the inverse transform (what the GPU rasterizer
/// and linear sampler do for the layer quad).
fn composite_texels(
    accum: &mut [[f32; 4]],
    frame_width: u32,
    frame_height: u32,
    texels: &Texels,
    opacity: f32,
    transform: LayerTransform,
) {
    if opacity <= 0.0 || texels.width == 0 || texels.height == 0 {
        return;
    }
    let (layer_w, layer_h) = (texels.width as f32, texels.height as f32);

    if transform.is_integer_translation() {
        let (offset_x, offset_y) = (transform.tx as i64, transform.ty as i64);
        for y in 0..texels.height {
            let out_y = y as i64 + offset_y;
            if out_y < 0 || out_y >= frame_height as i64 {
                continue;
            }
            for x in 0..texels.width {
                let out_x = x as i64 + offset_x;
                if out_x < 0 || out_x >= frame_width as i64 {
                    continue;
                }
                let mut source = texels.premultiplied_linear(x, y);
                if source[3] <= 0.0 && source[0] <= 0.0 && source[1] <= 0.0 && source[2] <= 0.0 {
                    continue;
                }
                source.iter_mut().for_each(|channel| *channel *= opacity);
                let index = (out_y as usize) * frame_width as usize + out_x as usize;
                color::source_over(&mut accum[index], source);
            }
        }
        return;
    }

    let det = transform.a * transform.d - transform.b * transform.c;
    if det.abs() < 1e-12 {
        return;
    }

    let corners = [
        transform.map(0.0, 0.0),
        transform.map(layer_w, 0.0),
        transform.map(0.0, layer_h),
        transform.map(layer_w, layer_h),
    ];
    let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
    let max_x = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let max_y = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::NEG_INFINITY, f32::max);
    let x_start = min_x.floor().max(0.0) as u32;
    let x_end = (max_x.ceil().max(0.0) as u32).min(frame_width);
    let y_start = min_y.floor().max(0.0) as u32;
    let y_end = (max_y.ceil().max(0.0) as u32).min(frame_height);

    // Coverage follows GPU rasterization rules: quad corners snapped to the rasterizer's
    // sub-pixel grid, pixel centers tested with edge functions and the top-left fill rule.
    let snap = |(x, y): (f32, f32)| {
        (
            (x * RASTER_SUBPIXELS).round() / RASTER_SUBPIXELS,
            (y * RASTER_SUBPIXELS).round() / RASTER_SUBPIXELS,
        )
    };
    let quad = [
        snap(corners[0]),
        snap(corners[1]),
        snap(corners[3]),
        snap(corners[2]),
    ];

    for out_y in y_start..y_end {
        let dy = out_y as f32 + 0.5 - transform.ty;
        for out_x in x_start..x_end {
            let center = (out_x as f32 + 0.5, out_y as f32 + 0.5);
            if !quad_covers(&quad, center) {
                continue;
            }
            let dx = out_x as f32 + 0.5 - transform.tx;
            let layer_x = ((transform.d * dx - transform.c * dy) / det).clamp(0.0, layer_w);
            let layer_y = ((-transform.b * dx + transform.a * dy) / det).clamp(0.0, layer_h);
            let mut source = texels.sample_bilinear(layer_x, layer_y);
            source.iter_mut().for_each(|channel| *channel *= opacity);
            let index = out_y as usize * frame_width as usize + out_x as usize;
            color::source_over(&mut accum[index], source);
        }
    }
}

/// Sub-pixel grid GPU rasterizers snap vertex positions to (8 bits, the D3D/Metal norm).
const RASTER_SUBPIXELS: f32 = 256.0;

/// Pixel-center coverage for a convex quad whose corners are in clockwise screen order
/// (y down), using edge functions with the top-left fill rule so shared edges between
/// adjacent quads are never covered twice.
fn quad_covers(quad: &[(f32, f32); 4], point: (f32, f32)) -> bool {
    for index in 0..4 {
        let (x0, y0) = quad[index];
        let (x1, y1) = quad[(index + 1) % 4];
        let edge = (x1 - x0) * (point.1 - y0) - (y1 - y0) * (point.0 - x0);
        if edge > 0.0 {
            continue;
        }
        if edge < 0.0 {
            return false;
        }
        // Exactly on the edge: only top edges (horizontal, interior below) and left edges
        // (heading up the screen) own their pixels.
        let is_top = y1 == y0 && x1 > x0;
        let is_left = y1 < y0;
        if !(is_top || is_left) {
            return false;
        }
    }
    true
}

fn render_procedural_texels(uniform: &ProceduralUniform, width: u32, height: u32) -> Texels {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&color::encode_texel(procedural_premultiplied(
                uniform, x, y,
            )));
        }
    }
    Texels {
        width,
        height,
        data,
    }
}

/// Rasterizes a text layer to straight-alpha sRGB (uniform text color, alpha = glyph
/// coverage combined with source-over), then converts to texels.
fn render_text_texels(layer: &TextLayer) -> Result<Texels> {
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
        return Ok(Texels::from_straight_srgb8(1, 1, vec![0; 4]));
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

    let width = width.max(1);
    let height = height.max(1);
    let mut coverage = vec![0.0_f32; (width * height) as usize];
    let text_color = layer.text.color;
    let alpha_base = text_color.a.clamp(0.0, 1.0);

    for glyph in glyphs {
        if glyph.width == 0 || glyph.height == 0 {
            continue;
        }
        let (_, bitmap) = font.rasterize_config(glyph.key);
        for row in 0..glyph.height {
            for col in 0..glyph.width {
                let source_alpha = alpha_base * bitmap[row * glyph.width + col] as f32 / 255.0;
                if source_alpha <= 0.0 {
                    continue;
                }
                let x = (glyph.x - min_x) as u32 + col as u32;
                let y = (glyph.y - min_y) as u32 + row as u32;
                if x < width && y < height {
                    let destination = &mut coverage[(y * width + x) as usize];
                    *destination = source_alpha + *destination * (1.0 - source_alpha);
                }
            }
        }
    }

    let rgb = [
        color::unorm8(text_color.r),
        color::unorm8(text_color.g),
        color::unorm8(text_color.b),
    ];
    let mut straight = Vec::with_capacity(coverage.len() * 4);
    for alpha in coverage {
        let alpha8 = color::unorm8(alpha);
        if alpha8 == 0 {
            straight.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            straight.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha8]);
        }
    }
    Ok(Texels::from_straight_srgb8(width, height, straight))
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
    time_base: TimeBase,
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
                format: LAYER_TEXEL_FORMAT,
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
        format: LAYER_TEXEL_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Create uniform buffer
    let init_context = ExpressionContext::new(0.0, time_base, params, seed);
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
        time_base,
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
    time_base: TimeBase,
) -> Result<GpuLayer> {
    let texels = render_text_texels(layer)?;
    let (layer_width, layer_height) = (texels.width, texels.height);

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
        format: LAYER_TEXEL_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue_write_texels(
        queue,
        &texture,
        &texels,
        &format!("text layer '{}'", layer.common.id),
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
        time_base,
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
    time_base: TimeBase,
) -> Result<GpuLayer> {
    let prepared = PreparedAsciiLayer::new(&layer.ascii, &layer.common.id)?;
    let layer_width = prepared.pixel_width();
    let layer_height = prepared.pixel_height();
    let is_static = prepared.is_static();
    let initial_texels = Texels::from_srgb_premultiplied_pixmap(&prepared.render_frame_pixmap(0)?);

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
        format: LAYER_TEXEL_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue_write_texels(
        queue,
        &texture,
        &initial_texels,
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
        time_base,
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

fn queue_write_texels(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    texels: &Texels,
    label: &str,
) -> Result<()> {
    let bytes_per_row = checked_bytes_per_row(texels.width, label)?.get();
    let rows_per_image = NonZeroU32::new(texels.height)
        .ok_or_else(|| anyhow!("{label} has invalid height {}", texels.height))?
        .get();

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &texels.data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(rows_per_image),
        },
        wgpu::Extent3d {
            width: texels.width,
            height: texels.height,
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
        // Re-pinned when the canonical linear-light pipeline landed (docs/COLOR_PIPELINE.md).
        assert_eq!(checksum, 15821704460225728760);
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
