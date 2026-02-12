//! GPU-based ASCII post-processing pipeline.
//!
//! Converts a composited RGBA frame into a quantized glyph-index field and
//! produces a glyph atlas render. Pipeline stages:
//!
//! 1. **Luma analysis** — input texture → cell-resolution luma texture
//! 2. **CellPassStack** — composable cell-resolution passes (edge weighting, dithering, etc.)
//! 3. **Quantize** — cell luma → glyph-index texture (normalized in R channel)
//! 4. **Atlas render** — glyph-index texture + atlas → full-resolution RGBA
//!
//! All passes are deterministic given the same inputs and configuration.

use std::path::Path;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use wgpu::util::DeviceExt;

use crate::post_process::PostGlobals;
use crate::schema::AsciiPostConfig;

// ---------------------------------------------------------------------------
// Per-shader parameter uniform (shared by all three ASCII passes)
// ---------------------------------------------------------------------------

/// Matches `AsciiParams` in all ASCII WGSL shaders.
/// 16 bytes, 16-byte aligned.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AsciiParams {
    pub grid_size: [f32; 2], // (cols, rows) as float
    pub ramp_len: f32,       // number of glyphs in ramp
    pub _pad: f32,
}

/// Matches `AtlasMetadata` in ascii_atlas_render.wgsl.
/// 16 bytes, 16-byte aligned.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct AtlasMetadata {
    cell_width: f32,
    cell_height: f32,
    atlas_columns: f32,
    _pad: f32,
}

// ---------------------------------------------------------------------------
// CellPass — cell-resolution texture → texture abstraction
// ---------------------------------------------------------------------------

/// Shared context for cell pass execution. Allows passes to bind globals and params.
pub struct CellPassContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub input_view: &'a wgpu::TextureView,
    pub output_view: &'a wgpu::TextureView,
    pub globals_buffer: &'a wgpu::Buffer,
    pub params_buffer: &'a wgpu::Buffer,
    pub sampler: &'a wgpu::Sampler,
}

/// A single cell-resolution pass. Reads from input texture, writes to output texture.
/// Pure texture → texture transform. Deterministic uniforms only.
///
/// Future passes (edge weighting, dithering, local contrast) will implement this.
pub trait CellPass: Send + Sync {
    /// Execute this pass: reads `input_view`, writes to `output_view`.
    fn execute(&self, _ctx: &mut CellPassContext<'_>) {
        // Default no-op for placeholder implementations
    }
}

// ---------------------------------------------------------------------------
// CellPassStack — ordered cell passes, applied sequentially
// ---------------------------------------------------------------------------

/// Ordered stack of cell-resolution passes. Applied sequentially between luma and quantize.
/// When empty, quantize reads directly from the luma texture (passthrough).
/// When non-empty, owns a scratch texture for pass output (ping-pong ready for future).
pub struct CellPassStack {
    passes: Vec<Box<dyn CellPass>>,
    scratch_texture: Option<wgpu::Texture>,
}

impl CellPassStack {
    /// Create an empty cell pass stack.
    pub fn empty() -> Self {
        Self {
            passes: Vec::new(),
            scratch_texture: None,
        }
    }

    /// Create a stack with passes. Allocates scratch texture for pass output.
    pub fn with_passes(
        device: &wgpu::Device,
        passes: Vec<Box<dyn CellPass>>,
        cols: u32,
        rows: u32,
        render_format: wgpu::TextureFormat,
    ) -> Self {
        let scratch_texture = if passes.is_empty() {
            None
        } else {
            Some(
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("ascii-cell-pass-scratch"),
                    size: wgpu::Extent3d {
                        width: cols,
                        height: rows,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: render_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                }),
            )
        };
        Self {
            passes,
            scratch_texture,
        }
    }

    /// Returns true if there are no cell passes to apply.
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// Returns the texture view that quantize should read from. When empty, returns None
    /// (caller uses luma). When non-empty, returns the scratch texture view.
    pub fn output_view(&self) -> Option<wgpu::TextureView> {
        self.scratch_texture
            .as_ref()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
    }

    /// Apply all passes sequentially. When empty, this is a no-op; the caller
    /// should use the luma texture directly for the quantize pass.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input_view: &wgpu::TextureView,
        globals_buffer: &wgpu::Buffer,
        params_buffer: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
    ) {
        let Some(scratch) = &self.scratch_texture else {
            return;
        };
        let output_view = scratch.create_view(&wgpu::TextureViewDescriptor::default());
        let mut ctx = CellPassContext {
            device,
            queue,
            encoder,
            input_view,
            output_view: &output_view,
            globals_buffer,
            params_buffer,
            sampler,
        };
        for pass in &self.passes {
            pass.execute(&mut ctx);
        }
    }
}

impl Default for CellPassStack {
    fn default() -> Self {
        Self::empty()
    }
}

// ---------------------------------------------------------------------------
// EdgeBoostCellPass — first real cell pass
// ---------------------------------------------------------------------------

const ASCII_CELL_EDGE_BOOST_WGSL: &str =
    include_str!("../shaders/wgsl/ascii_cell_edge_boost.wgsl");

/// Edge boost parameters (hardcoded; no schema).
const EDGE_GAIN: f32 = 2.0;
const BOOST: f32 = 0.25;

/// Cell pass that estimates edge strength and darkens edges for crisper ASCII silhouettes.
pub struct EdgeBoostCellPass {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl EdgeBoostCellPass {
    /// Build the edge boost pass. Uses the same bind layout as luma/quantize.
    pub fn new(
        device: &wgpu::Device,
        quantize_bgl: &wgpu::BindGroupLayout,
        render_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ascii-cell-edge-boost"),
            source: wgpu::ShaderSource::Wgsl(ASCII_CELL_EDGE_BOOST_WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ascii-cell-edge-boost-layout"),
            bind_group_layouts: &[quantize_bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ascii-cell-edge-boost-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        // Create owned bind group layout for bind group creation (same layout as quantize)
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ascii-edge-boost-bgl"),
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
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        Ok(Self {
            pipeline,
            bind_group_layout,
        })
    }
}

impl CellPass for EdgeBoostCellPass {
    fn execute(&self, ctx: &mut CellPassContext<'_>) {
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ascii-edge-boost-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(ctx.input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(ctx.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: ctx.globals_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: ctx.params_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ascii-cell-edge-boost-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.output_view,
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

// ---------------------------------------------------------------------------
// Pure functions for edge boost math (unit tested)
// ---------------------------------------------------------------------------

/// Compute edge strength from finite-difference gradient. Deterministic.
pub fn edge_strength(dx: f32, dy: f32, edge_gain: f32) -> f32 {
    (dx + dy) * edge_gain
}

/// Apply edge boost to luma. Deterministic.
pub fn apply_edge_boost(luma: f32, edge: f32, boost: f32) -> f32 {
    (luma - edge * boost).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Embedded WGSL sources
// ---------------------------------------------------------------------------

const ASCII_LUMA_WGSL: &str = include_str!("../shaders/wgsl/ascii_luma.wgsl");
const ASCII_QUANTIZE_WGSL: &str = include_str!("../shaders/wgsl/ascii_quantize.wgsl");
const ASCII_ATLAS_RENDER_WGSL: &str = include_str!("../shaders/wgsl/ascii_atlas_render.wgsl");

const GLYPH_ATLAS_PATH: &str = "assets/glyph_atlas/geist_pixel_line.png";
const GLYPH_ATLAS_META_PATH: &str = "assets/glyph_atlas/geist_pixel_line.meta.json";

// ---------------------------------------------------------------------------
// AsciiPipeline
// ---------------------------------------------------------------------------

/// Holds GPU resources for the ASCII pipeline.
#[allow(dead_code)] // Config fields stored for future atlas/terminal phases.
pub struct AsciiPipeline {
    cols: u32,
    rows: u32,
    ramp_len: u32,

    // Cell pass stack (luma → optional effects → quantize input)
    cell_pass_stack: CellPassStack,

    // Intermediate textures at cell resolution (cols x rows)
    luma_texture: wgpu::Texture,
    glyph_texture: wgpu::Texture,

    // Atlas render output texture at full resolution
    atlas_output_texture: wgpu::Texture,
    atlas_texture: wgpu::Texture,

    // Pipelines
    luma_pipeline: wgpu::RenderPipeline,
    quantize_pipeline: wgpu::RenderPipeline,
    atlas_pipeline: wgpu::RenderPipeline,

    // Bind group layouts
    luma_bgl: wgpu::BindGroupLayout,
    quantize_bgl: wgpu::BindGroupLayout,
    atlas_bgl: wgpu::BindGroupLayout,

    // Shared resources
    sampler: wgpu::Sampler,
    nearest_sampler: wgpu::Sampler,
    globals_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    atlas_metadata_buffer: wgpu::Buffer,
    params: AsciiParams,
    atlas_metadata: AtlasMetadata,

    // Full output dimensions
    output_width: u32,
    output_height: u32,
}

impl AsciiPipeline {
    /// Build the ASCII pipeline from a validated configuration.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &AsciiPostConfig,
        output_width: u32,
        output_height: u32,
        render_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let cols = config.cols;
        let rows = config.rows;
        let ramp_len = config.ramp_len() as u32;

        let params = AsciiParams {
            grid_size: [cols as f32, rows as f32],
            ramp_len: ramp_len as f32,
            _pad: 0.0,
        };

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ascii-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ascii-nearest-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ascii-globals"),
            contents: bytemuck::bytes_of(&PostGlobals::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ascii-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let atlas_path = repo_root.join(GLYPH_ATLAS_PATH);
        let atlas_img = ImageReader::open(&atlas_path)
            .with_context(|| format!("failed to open glyph atlas {}", atlas_path.display()))?
            .decode()
            .context("failed to decode glyph atlas image")?
            .into_rgba8();

        let atlas_meta_path = repo_root.join(GLYPH_ATLAS_META_PATH);
        let atlas_meta_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&atlas_meta_path).with_context(|| {
                format!(
                    "failed to read atlas metadata {}",
                    atlas_meta_path.display()
                )
            })?)
            .context("failed to parse atlas metadata JSON")?;
        let atlas_metadata = AtlasMetadata {
            cell_width: atlas_meta_json["cell_width"]
                .as_u64()
                .context("atlas meta missing cell_width")? as f32,
            cell_height: atlas_meta_json["cell_height"]
                .as_u64()
                .context("atlas meta missing cell_height")? as f32,
            atlas_columns: atlas_meta_json["atlas_columns"]
                .as_u64()
                .context("atlas meta missing atlas_columns")? as f32,
            _pad: 0.0,
        };

        let atlas_metadata_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ascii-atlas-metadata"),
            contents: bytemuck::bytes_of(&atlas_metadata),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let (atlas_width, atlas_height) = atlas_img.dimensions();
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ascii-atlas-texture"),
            size: wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes_per_row = atlas_width * 4;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            atlas_img.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(atlas_height),
            },
            wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
        );

        // Create intermediate textures at cell resolution
        let cell_size = wgpu::Extent3d {
            width: cols,
            height: rows,
            depth_or_array_layers: 1,
        };

        let luma_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ascii-luma"),
            size: cell_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: render_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let glyph_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ascii-glyph"),
            size: cell_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: render_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        // Atlas render output at full resolution
        let atlas_output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ascii-atlas-output"),
            size: wgpu::Extent3d {
                width: output_width,
                height: output_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: render_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // Build bind group layout (identical for all three passes):
        // @binding(0) texture_2d<f32>, @binding(1) sampler,
        // @binding(2) PostGlobals, @binding(3) AsciiParams
        let make_bgl = |label: &str| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
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
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            })
        };

        let luma_bgl = make_bgl("ascii-luma-bgl");
        let quantize_bgl = make_bgl("ascii-quantize-bgl");

        let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ascii-atlas-bgl"),
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
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // Build pipelines
        let make_pipeline = |label: &str,
                             wgsl: &str,
                             bgl: &wgpu::BindGroupLayout|
         -> Result<wgpu::RenderPipeline> {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });

            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("{label}-layout")),
                bind_group_layouts: &[bgl],
                push_constant_ranges: &[],
            });

            Ok(
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&format!("{label}-pipeline")),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: "vs_main",
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: "fs_main",
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: render_format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                }),
            )
        };

        let luma_pipeline = make_pipeline("ascii-luma", ASCII_LUMA_WGSL, &luma_bgl)
            .context("failed to create ASCII luma pipeline")?;
        let quantize_pipeline = make_pipeline("ascii-quantize", ASCII_QUANTIZE_WGSL, &quantize_bgl)
            .context("failed to create ASCII quantize pipeline")?;
        let atlas_pipeline = make_pipeline("ascii-atlas", ASCII_ATLAS_RENDER_WGSL, &atlas_bgl)
            .context("failed to create ASCII atlas pipeline")?;

        Ok(Self {
            cols,
            rows,
            ramp_len,
            cell_pass_stack: CellPassStack::empty(),
            luma_texture,
            glyph_texture,
            atlas_output_texture,
            atlas_texture,
            luma_pipeline,
            quantize_pipeline,
            atlas_pipeline,
            luma_bgl,
            quantize_bgl,
            atlas_bgl,
            sampler,
            nearest_sampler,
            globals_buffer,
            params_buffer,
            atlas_metadata_buffer,
            params,
            atlas_metadata,
            output_width,
            output_height,
        })
    }

    /// Apply the ASCII pipeline: input texture → debug output texture.
    ///
    /// After this call, the debug visualization is in `self.debug_texture`.
    /// The caller should copy it back to the output texture.
    pub fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input_texture: &wgpu::Texture,
        frame_index: u32,
        fps: u32,
        seed: u64,
    ) {
        // Update globals
        let globals = PostGlobals {
            resolution: [self.output_width as f32, self.output_height as f32],
            time: frame_index as f32 / fps as f32,
            frame_index,
            seed: seed as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let input_view = input_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let luma_view = self
            .luma_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let glyph_view = self
            .glyph_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_output_view = self
            .atlas_output_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_view = self
            .atlas_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // --- Pass 1: Luma analysis (full res input → cell res luma) ---
        let luma_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ascii-luma-bg"),
            layout: &self.luma_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.globals_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.params_buffer.as_entire_binding(),
                },
            ],
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ascii-luma-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &luma_view,
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
            pass.set_pipeline(&self.luma_pipeline);
            pass.set_bind_group(0, &luma_bg, &[]);
            pass.draw(0..3, 0..1);
        }

        // --- CellPassStack: optional cell-resolution effects (empty by default) ---
        self.cell_pass_stack.apply(
            device, queue, encoder, &luma_view,
            &luma_view, // when empty, no-op; when populated, would use scratch texture
        );

        // --- Pass 2: Quantize (cell res luma → cell res glyph index) ---
        let quantize_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ascii-quantize-bg"),
            layout: &self.quantize_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&luma_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.globals_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.params_buffer.as_entire_binding(),
                },
            ],
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ascii-quantize-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &glyph_view,
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
            pass.set_pipeline(&self.quantize_pipeline);
            pass.set_bind_group(0, &quantize_bg, &[]);
            pass.draw(0..3, 0..1);
        }

        // --- Pass 3: Atlas render (cell res glyph + atlas → full res RGBA) ---
        let atlas_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ascii-atlas-bg"),
            layout: &self.atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&glyph_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.nearest_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.globals_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.nearest_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: self.atlas_metadata_buffer.as_entire_binding(),
                },
            ],
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ascii-atlas-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &atlas_output_view,
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
            pass.set_pipeline(&self.atlas_pipeline);
            pass.set_bind_group(0, &atlas_bg, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Copy the atlas render result back to the output texture.
    pub fn copy_debug_to_output(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output_texture: &wgpu::Texture,
    ) {
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: &self.atlas_output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
                texture: output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: self.output_width,
                height: self.output_height,
                depth_or_array_layers: 1,
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Pure helper functions for glyph mapping (used in tests, docs)
// ---------------------------------------------------------------------------

/// Map a luminance value [0, 1] to a glyph index [0, ramp_len-1].
///
/// Dark pixels (low luma) map to high indices (dense characters).
/// Light pixels (high luma) map to low indices (sparse characters).
///
/// Formula: `clamp(floor((1.0 - luma) * ramp_len), 0, ramp_len - 1)`
pub fn luma_to_glyph_index(luma: f32, ramp_len: u32) -> u32 {
    let n = ramp_len as f32;
    let id = ((1.0 - luma.clamp(0.0, 1.0)) * n).floor();
    (id as u32).min(ramp_len - 1)
}

/// Convert a glyph index [0, ramp_len-1] back to a debug grayscale value [0, 1].
///
/// Formula: `1.0 - (id / (ramp_len - 1))`
pub fn glyph_index_to_debug_gray(id: u32, ramp_len: u32) -> f32 {
    if ramp_len <= 1 {
        return 0.5;
    }
    1.0 - (id as f32 / (ramp_len - 1) as f32)
}

/// Compute Rec.709 luminance from linear RGB.
pub fn rec709_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_params_is_16_bytes() {
        assert_eq!(
            std::mem::size_of::<AsciiParams>(),
            16,
            "AsciiParams must be 16 bytes to match WGSL struct"
        );
    }

    #[test]
    fn luma_to_glyph_maps_white_to_zero() {
        // luma=1.0 (white) → id=0 (lightest glyph, e.g. space)
        assert_eq!(luma_to_glyph_index(1.0, 10), 0);
    }

    #[test]
    fn luma_to_glyph_maps_black_to_max() {
        // luma=0.0 (black) → id=9 (densest glyph, e.g. @)
        assert_eq!(luma_to_glyph_index(0.0, 10), 9);
    }

    #[test]
    fn luma_to_glyph_maps_midtone_correctly() {
        // luma=0.5 → (1.0 - 0.5) * 10 = 5.0 → floor = 5
        assert_eq!(luma_to_glyph_index(0.5, 10), 5);
    }

    #[test]
    fn luma_to_glyph_clamps_out_of_range() {
        assert_eq!(luma_to_glyph_index(-0.5, 10), 9);
        assert_eq!(luma_to_glyph_index(1.5, 10), 0);
    }

    #[test]
    fn luma_to_glyph_with_ramp_len_2() {
        assert_eq!(luma_to_glyph_index(0.0, 2), 1);
        assert_eq!(luma_to_glyph_index(0.49, 2), 1);
        assert_eq!(luma_to_glyph_index(0.51, 2), 0);
        assert_eq!(luma_to_glyph_index(1.0, 2), 0);
    }

    #[test]
    fn debug_gray_roundtrips_identity_at_boundaries() {
        let ramp = 10u32;
        // id=0 → gray=1.0 (white)
        assert!((glyph_index_to_debug_gray(0, ramp) - 1.0).abs() < f32::EPSILON);
        // id=9 → gray=0.0 (black)
        assert!((glyph_index_to_debug_gray(9, ramp) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn debug_gray_midpoint() {
        // id=5 out of 10 → gray = 1.0 - 5/9 ≈ 0.444
        let gray = glyph_index_to_debug_gray(5, 10);
        assert!((gray - (1.0 - 5.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn rec709_luminance_for_pure_white() {
        let luma = rec709_luminance(1.0, 1.0, 1.0);
        assert!((luma - 1.0).abs() < 0.001);
    }

    #[test]
    fn rec709_luminance_for_pure_green() {
        let luma = rec709_luminance(0.0, 1.0, 0.0);
        assert!((luma - 0.7152).abs() < 0.001);
    }

    #[test]
    fn rec709_luminance_for_pure_red() {
        let luma = rec709_luminance(1.0, 0.0, 0.0);
        assert!((luma - 0.2126).abs() < 0.001);
    }

    #[test]
    fn rec709_luminance_for_pure_blue() {
        let luma = rec709_luminance(0.0, 0.0, 1.0);
        assert!((luma - 0.0722).abs() < 0.001);
    }

    #[test]
    fn embedded_wgsl_sources_are_nonempty() {
        assert!(!ASCII_LUMA_WGSL.is_empty());
        assert!(!ASCII_QUANTIZE_WGSL.is_empty());
        assert!(!ASCII_ATLAS_RENDER_WGSL.is_empty());
    }

    #[test]
    fn wgsl_sources_contain_entry_points() {
        assert!(ASCII_LUMA_WGSL.contains("fn vs_main"));
        assert!(ASCII_LUMA_WGSL.contains("fn fs_main"));
        assert!(ASCII_QUANTIZE_WGSL.contains("fn vs_main"));
        assert!(ASCII_QUANTIZE_WGSL.contains("fn fs_main"));
        assert!(ASCII_ATLAS_RENDER_WGSL.contains("fn vs_main"));
        assert!(ASCII_ATLAS_RENDER_WGSL.contains("fn fs_main"));
    }

    #[test]
    fn wgsl_sources_declare_ascii_params() {
        assert!(ASCII_LUMA_WGSL.contains("struct AsciiParams"));
        assert!(ASCII_QUANTIZE_WGSL.contains("struct AsciiParams"));
        assert!(ASCII_ATLAS_RENDER_WGSL.contains("struct AsciiParams"));
    }

    #[test]
    fn wgsl_sources_declare_post_globals() {
        assert!(ASCII_LUMA_WGSL.contains("struct PostGlobals"));
        assert!(ASCII_QUANTIZE_WGSL.contains("struct PostGlobals"));
        assert!(ASCII_ATLAS_RENDER_WGSL.contains("struct PostGlobals"));
    }

    /// Verify glyph index math: glyph_index = round(normalized_value * (N - 1))
    #[test]
    fn normalized_glyph_id_to_index_math() {
        let ramp_len = 10u32;
        let n = ramp_len as f32;
        for idx in 0..ramp_len {
            let normalized = if ramp_len > 1 {
                idx as f32 / (ramp_len - 1) as f32
            } else {
                0.5
            };
            let recovered = (normalized * (n - 1.0)).round();
            assert!(
                (recovered - idx as f32).abs() < 0.001,
                "idx={idx} normalized={normalized} recovered={recovered}"
            );
        }
    }

    #[test]
    fn glyph_mapping_is_deterministic() {
        // Same input always produces same output
        for i in 0..100 {
            let luma = i as f32 / 100.0;
            let a = luma_to_glyph_index(luma, 10);
            let b = luma_to_glyph_index(luma, 10);
            assert_eq!(a, b, "glyph mapping must be deterministic for luma={luma}");
        }
    }

    #[test]
    fn glyph_mapping_covers_full_range() {
        let ramp = 10u32;
        let mut seen = std::collections::HashSet::new();
        for i in 0..1000 {
            let luma = i as f32 / 999.0;
            seen.insert(luma_to_glyph_index(luma, ramp));
        }
        assert_eq!(
            seen.len(),
            ramp as usize,
            "all glyph indices should be reachable"
        );
    }

    // --- CellPassStack (placeholder: pipeline executes correctly when stack is empty) ---

    #[test]
    fn cell_pass_stack_empty_by_default() {
        let stack = CellPassStack::empty();
        assert!(stack.is_empty(), "new CellPassStack should be empty");
    }

    #[test]
    fn cell_pass_stack_default_is_empty() {
        let stack = CellPassStack::default();
        assert!(stack.is_empty(), "CellPassStack::default() should be empty");
    }
}
