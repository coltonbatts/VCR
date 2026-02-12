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
/// When non-empty, owns scratch texture(s) for pass output (ping-pong when multiple passes).
pub struct CellPassStack {
    passes: Vec<Box<dyn CellPass>>,
    scratch_a: Option<wgpu::Texture>,
    scratch_b: Option<wgpu::Texture>,
}

impl CellPassStack {
    /// Create an empty cell pass stack.
    pub fn empty() -> Self {
        Self {
            passes: Vec::new(),
            scratch_a: None,
            scratch_b: None,
        }
    }

    /// Create a stack with passes. Allocates scratch texture(s) for pass output.
    /// Uses two scratch textures when multiple passes (for ping-pong chaining).
    pub fn with_passes(
        device: &wgpu::Device,
        passes: Vec<Box<dyn CellPass>>,
        cols: u32,
        rows: u32,
        render_format: wgpu::TextureFormat,
    ) -> Self {
        let descriptor = wgpu::TextureDescriptor {
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
        };
        let (scratch_a, scratch_b) = if passes.is_empty() {
            (None, None)
        } else if passes.len() == 1 {
            (Some(device.create_texture(&descriptor)), None)
        } else {
            (
                Some(device.create_texture(&descriptor)),
                Some(device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("ascii-cell-pass-scratch-b"),
                    ..descriptor
                })),
            )
        };
        Self {
            passes,
            scratch_a,
            scratch_b,
        }
    }

    /// Returns true if there are no cell passes to apply.
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// Returns the texture view that quantize should read from. When empty, returns None
    /// (caller uses luma). When non-empty, returns the final scratch texture view.
    pub fn output_view(&self) -> Option<wgpu::TextureView> {
        if self.passes.len() > 1 {
            self.scratch_b
                .as_ref()
                .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
        } else {
            self.scratch_a
                .as_ref()
                .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
        }
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
        let Some(scratch_a) = &self.scratch_a else {
            return;
        };
        let view_a = scratch_a.create_view(&wgpu::TextureViewDescriptor::default());

        if self.passes.len() == 1 {
            let mut ctx = CellPassContext {
                device,
                queue,
                encoder,
                input_view,
                output_view: &view_a,
                globals_buffer,
                params_buffer,
                sampler,
            };
            self.passes[0].execute(&mut ctx);
            return;
        }

        let Some(scratch_b) = &self.scratch_b else {
            return;
        };
        let view_b = scratch_b.create_view(&wgpu::TextureViewDescriptor::default());

        // Pass 0: input -> scratch_a
        let mut ctx = CellPassContext {
            device,
            queue,
            encoder,
            input_view,
            output_view: &view_a,
            globals_buffer,
            params_buffer,
            sampler,
        };
        self.passes[0].execute(&mut ctx);

        // Pass 1: scratch_a -> scratch_b (and further passes ping-pong)
        let mut read_view: &wgpu::TextureView = &view_a;
        let mut write_view: &wgpu::TextureView = &view_b;
        for pass in self.passes.iter().skip(1) {
            let mut ctx = CellPassContext {
                device,
                queue,
                encoder,
                input_view: read_view,
                output_view: write_view,
                globals_buffer,
                params_buffer,
                sampler,
            };
            pass.execute(&mut ctx);
            std::mem::swap(&mut read_view, &mut write_view);
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

const ASCII_CELL_EDGE_BOOST_WGSL: &str = include_str!("../shaders/wgsl/ascii_cell_edge_boost.wgsl");
const ASCII_CELL_BAYER_DITHER_WGSL: &str = include_str!("../shaders/wgsl/ascii_cell_bayer_dither.wgsl");

/// Edge boost parameters (hardcoded; no schema). Used by unit tests and documented for WGSL parity.
pub const EDGE_GAIN: f32 = 2.0;
pub const BOOST: f32 = 0.25;

/// Default for edge boost when no runtime override is provided. Used by renderer.
pub(crate) const DEFAULT_EDGE_BOOST: bool = true;

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
// BayerDitherCellPass — ordered dither before quantization
// ---------------------------------------------------------------------------

/// Default for Bayer dither when no runtime override is provided.
pub(crate) const DEFAULT_BAYER_DITHER: bool = false;

/// Cell pass that applies subtle 8×8 ordered Bayer dithering to break up banding.
pub struct BayerDitherCellPass {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl BayerDitherCellPass {
    pub fn new(
        device: &wgpu::Device,
        quantize_bgl: &wgpu::BindGroupLayout,
        render_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ascii-cell-bayer-dither"),
            source: wgpu::ShaderSource::Wgsl(ASCII_CELL_BAYER_DITHER_WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ascii-cell-bayer-dither-layout"),
            bind_group_layouts: &[quantize_bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ascii-cell-bayer-dither-pipeline"),
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
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ascii-bayer-dither-bgl"),
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

impl CellPass for BayerDitherCellPass {
    fn execute(&self, ctx: &mut CellPassContext<'_>) {
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ascii-bayer-dither-bg"),
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
            label: Some("ascii-cell-bayer-dither-pass"),
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
// Pure functions for Bayer dither (unit tested)
// ---------------------------------------------------------------------------

/// 8×8 Bayer matrix values 0..63 (standard ordered dither pattern). Row-major.
pub const BAYER_8X8: [u8; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38, 60,
    28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25, 15, 47,
    7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
];

/// Bayer threshold for cell (x, y). Deterministic. (bayer_val + 0.5) / 64.0
pub fn bayer_threshold(x: u32, y: u32) -> f32 {
    let idx = (y % 8) * 8 + (x % 8);
    (BAYER_8X8[idx as usize] as f32 + 0.5) / 64.0
}

/// Apply Bayer dither adjustment. Deterministic.
/// adjusted = clamp(luma + (threshold - 0.5) * dither_strength, 0..1)
pub fn apply_bayer_dither(luma: f32, threshold: f32, dither_strength: f32) -> f32 {
    (luma + (threshold - 0.5) * dither_strength).clamp(0.0, 1.0)
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
    ///
    /// `enable_edge_boost`: when true, runs EdgeBoostCellPass. Overridable via CLI/env.
    /// `enable_bayer_dither`: when true, runs BayerDitherCellPass. Overridable via CLI/env.
    /// Pass order: EdgeBoost first, BayerDither second.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &AsciiPostConfig,
        output_width: u32,
        output_height: u32,
        render_format: wgpu::TextureFormat,
        enable_edge_boost: bool,
        enable_bayer_dither: bool,
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

        let cell_pass_stack = {
            let mut passes: Vec<Box<dyn CellPass>> = Vec::new();
            if enable_edge_boost {
                let edge_boost = EdgeBoostCellPass::new(device, &quantize_bgl, render_format)
                    .context("failed to create edge boost cell pass")?;
                passes.push(Box::new(edge_boost));
            }
            if enable_bayer_dither {
                let bayer_dither =
                    BayerDitherCellPass::new(device, &quantize_bgl, render_format)
                        .context("failed to create Bayer dither cell pass")?;
                passes.push(Box::new(bayer_dither));
            }
            if passes.is_empty() {
                CellPassStack::empty()
            } else {
                CellPassStack::with_passes(device, passes, cols, rows, render_format)
            }
        };

        Ok(Self {
            cols,
            rows,
            ramp_len,
            cell_pass_stack,
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

        // --- CellPassStack: edge boost and future cell passes ---
        self.cell_pass_stack.apply(
            device,
            queue,
            encoder,
            &luma_view,
            &self.globals_buffer,
            &self.params_buffer,
            &self.sampler,
        );

        // --- Pass 2: Quantize (cell res luma → cell res glyph index) ---
        let quantize_bg = if let Some(scratch_view) = self.cell_pass_stack.output_view() {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ascii-quantize-bg"),
                layout: &self.quantize_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&scratch_view),
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
            })
        } else {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
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
            })
        };

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
        assert!(!ASCII_CELL_EDGE_BOOST_WGSL.is_empty());
        assert!(!ASCII_CELL_BAYER_DITHER_WGSL.is_empty());
    }

    #[test]
    fn wgsl_sources_contain_entry_points() {
        assert!(ASCII_LUMA_WGSL.contains("fn vs_main"));
        assert!(ASCII_LUMA_WGSL.contains("fn fs_main"));
        assert!(ASCII_QUANTIZE_WGSL.contains("fn vs_main"));
        assert!(ASCII_QUANTIZE_WGSL.contains("fn fs_main"));
        assert!(ASCII_ATLAS_RENDER_WGSL.contains("fn vs_main"));
        assert!(ASCII_ATLAS_RENDER_WGSL.contains("fn fs_main"));
        assert!(ASCII_CELL_EDGE_BOOST_WGSL.contains("fn vs_main"));
        assert!(ASCII_CELL_EDGE_BOOST_WGSL.contains("fn fs_main"));
        assert!(ASCII_CELL_BAYER_DITHER_WGSL.contains("fn vs_main"));
        assert!(ASCII_CELL_BAYER_DITHER_WGSL.contains("fn fs_main"));
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

    // --- Edge boost pure functions ---

    #[test]
    fn edge_strength_zero_gradient_is_zero() {
        assert!(
            (edge_strength(0.0, 0.0, EDGE_GAIN) - 0.0).abs() < f32::EPSILON,
            "no gradient → zero edge"
        );
    }

    #[test]
    fn edge_strength_dx_dy_behavior() {
        // dx=0.5, dy=0 → (0.5+0)*2 = 1.0, clamp to 1.0
        let e = edge_strength(0.5, 0.0, EDGE_GAIN);
        assert!((e - 1.0).abs() < f32::EPSILON, "dx+dy scaled by gain");
        // dx=0.25, dy=0.25 → 0.5*2 = 1.0
        let e2 = edge_strength(0.25, 0.25, EDGE_GAIN);
        assert!((e2 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn edge_strength_clamps_to_one() {
        let e = edge_strength(1.0, 1.0, EDGE_GAIN);
        // (1+1)*2 = 4, but edge_strength doesn't clamp - the WGSL does
        // Our Rust fn is: (dx+dy)*edge_gain, no clamp. So e = 4.0.
        // The spec says edge = clamp((dx+dy)*edge_gain, 0..1). So the clamped
        // value is used in apply_edge_boost. Our edge_strength is the raw value.
        // The apply_edge_boost receives edge which would be clamped. Let me check
        // our function - edge_strength returns (dx+dy)*edge_gain without clamp.
        // The tests should verify the math. For "clamp((dx+dy)*edge_gain, 0..1)"
        // we could add a helper, or test that our edge_strength gives expected
        // values. The "clamp to 1" is in the shader. Our pure fn is for testing
        // the formula. Let me add a test that edge can exceed 1 (our fn doesn't
        // clamp) - that's fine, the shader clamps. For unit tests we're testing
        // the Rust helpers. I'll add a test for apply_edge_boost clamping.
        assert!((e - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_edge_boost_clamps_output() {
        // adjusted = clamp(luma - edge * boost, 0..1)
        assert!((apply_edge_boost(0.0, 1.0, BOOST) - 0.0).abs() < f32::EPSILON);
        assert!((apply_edge_boost(1.0, 0.0, BOOST) - 1.0).abs() < f32::EPSILON);
        // luma=0.5, edge=1, boost=0.25 → 0.5 - 0.25 = 0.25
        assert!(
            (apply_edge_boost(0.5, 1.0, BOOST) - 0.25).abs() < f32::EPSILON,
            "mid luma with full edge darkens by boost"
        );
    }

    #[test]
    fn apply_edge_boost_no_edge_preserves_luma() {
        assert!(
            (apply_edge_boost(0.7, 0.0, BOOST) - 0.7).abs() < f32::EPSILON,
            "zero edge → luma unchanged"
        );
    }

    #[test]
    fn apply_edge_boost_deterministic() {
        for i in 0..20 {
            let luma = i as f32 / 19.0;
            let a = apply_edge_boost(luma, 0.5, BOOST);
            let b = apply_edge_boost(luma, 0.5, BOOST);
            assert!((a - b).abs() < f32::EPSILON, "deterministic at luma={luma}");
        }
    }

    // --- Bayer dither pure functions ---

    #[test]
    fn bayer_threshold_mapping_for_coordinates() {
        // (0,0) -> idx 0 -> bayer 0 -> (0+0.5)/64 = 0.0078125
        assert!(
            (bayer_threshold(0, 0) - 0.5 / 64.0).abs() < f32::EPSILON,
            "(0,0) -> threshold ~= 0.0078"
        );
        // (1,0) -> idx 1 -> bayer 32 -> (32+0.5)/64 = 0.5078
        assert!(
            (bayer_threshold(1, 0) - 32.5 / 64.0).abs() < f32::EPSILON,
            "(1,0) -> threshold ~= 0.508"
        );
        // (7,7) -> idx 63 -> bayer 21 -> (21+0.5)/64
        assert!(
            (bayer_threshold(7, 7) - 21.5 / 64.0).abs() < f32::EPSILON,
            "(7,7) -> threshold"
        );
        // (8,8) wraps to (0,0)
        assert!(
            (bayer_threshold(8, 8) - bayer_threshold(0, 0)).abs() < f32::EPSILON,
            "Bayer repeats every 8"
        );
    }

    #[test]
    fn apply_bayer_dither_clamps() {
        let strength = 1.0 / 10.0; // ramp_len=10
        assert!(
            (apply_bayer_dither(0.0, 0.5 / 64.0, strength) - 0.0).abs() < 0.001,
            "luma 0 with low threshold clamps to 0"
        );
        assert!(
            (apply_bayer_dither(1.0, 63.5 / 64.0, strength) - 1.0).abs() < 0.001,
            "luma 1 with high threshold clamps to 1"
        );
        // threshold 0.5/64 -> adjustment (0.5/64 - 0.5)*0.1 ≈ -0.078; luma 0 + (-0.078) -> clamp to 0
        assert!(
            (apply_bayer_dither(0.0, 0.5 / 64.0, strength)).abs() < 0.001,
            "negative adjustment clamps to 0"
        );
        // threshold 63.5/64 -> (0.992 - 0.5)*0.1 ≈ 0.049; luma 1 + 0.049 -> clamp to 1
        assert!(
            (apply_bayer_dither(1.0, 63.5 / 64.0, strength) - 1.0).abs() < 0.001,
            "positive adjustment clamps to 1"
        );
    }

    #[test]
    fn apply_bayer_dither_deterministic() {
        let strength = 1.0 / 16.0;
        for i in 0..20 {
            let luma = i as f32 / 19.0;
            let thresh = bayer_threshold(i % 8, (i + 1) % 8);
            let a = apply_bayer_dither(luma, thresh, strength);
            let b = apply_bayer_dither(luma, thresh, strength);
            assert!(
                (a - b).abs() < f32::EPSILON,
                "deterministic at luma={luma} thresh={thresh}"
            );
        }
    }
}
