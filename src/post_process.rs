//! Post-processing shader primitive system.
//!
//! Provides a composable pipeline of texture → texture shader passes that run
//! after layer compositing. Each pass reads from an input texture, applies a
//! WGSL shader with typed parameters, and writes to an output texture.
//!
//! Architecture:
//!   - [`PostGlobals`] — shared uniform available to every shader (resolution, time, frame, seed).
//!   - [`ShaderPass`] — one GPU pipeline + bind groups + parameter buffer for a single effect.
//!   - [`PostStack`] — ordered `Vec<ShaderPass>`, applied sequentially with ping-pong textures.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::schema::{PostEffect, PostEffectKind};

// ---------------------------------------------------------------------------
// Global uniform (shared across all post shaders)
// ---------------------------------------------------------------------------

/// Matches the `PostGlobals` struct declared in every post-processing WGSL shader.
/// 32 bytes, 16-byte aligned (two vec4-sized rows).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PostGlobals {
    pub resolution: [f32; 2],
    pub time: f32,
    pub frame_index: u32,
    pub seed: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

// ---------------------------------------------------------------------------
// Per-shader parameter uniforms
// ---------------------------------------------------------------------------

/// Levels correction parameters. Matches `LevelsParams` in levels.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LevelsParams {
    pub gamma: f32,
    pub lift: f32,
    pub gain: f32,
    pub _pad: f32,
}

impl Default for LevelsParams {
    fn default() -> Self {
        Self {
            gamma: 1.0,
            lift: 0.0,
            gain: 1.0,
            _pad: 0.0,
        }
    }
}

/// Sobel edge-detection parameters. Matches `SobelParams` in sobel.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SobelParams {
    pub strength: f32,
    pub _pad0: f32,
    pub _pad1: f32,
    pub _pad2: f32,
}

impl Default for SobelParams {
    fn default() -> Self {
        Self {
            strength: 1.0,
            _pad0: 0.0,
            _pad1: 0.0,
            _pad2: 0.0,
        }
    }
}

/// Typed parameter payload for a single shader pass.
#[derive(Debug, Clone, Copy)]
pub enum ShaderParams {
    None,
    Levels(LevelsParams),
    Sobel(SobelParams),
}

impl ShaderParams {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::None => &[],
            Self::Levels(p) => bytemuck::bytes_of(p),
            Self::Sobel(p) => bytemuck::bytes_of(p),
        }
    }

    fn byte_len(&self) -> usize {
        self.as_bytes().len()
    }
}

// ---------------------------------------------------------------------------
// Embedded WGSL sources
// ---------------------------------------------------------------------------

const PASSTHROUGH_WGSL: &str = include_str!("../shaders/wgsl/passthrough.wgsl");
const LEVELS_WGSL: &str = include_str!("../shaders/wgsl/levels.wgsl");
const SOBEL_WGSL: &str = include_str!("../shaders/wgsl/sobel.wgsl");

// ---------------------------------------------------------------------------
// ShaderPass
// ---------------------------------------------------------------------------

/// A single post-processing pass: holds a compiled pipeline, bind group layout,
/// a globals uniform buffer, and an optional per-shader parameter buffer.
pub struct ShaderPass {
    pub label: String,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    globals_buffer: wgpu::Buffer,
    params_buffer: Option<wgpu::Buffer>,
    params: ShaderParams,
}

impl ShaderPass {
    /// Build a shader pass from a WGSL source string and typed parameters.
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        wgsl_source: &str,
        params: ShaderParams,
        render_format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
        });

        // Build bind group layout entries: texture, sampler, globals, [params]
        let mut layout_entries = vec![
            // @binding(0) input texture
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
            // @binding(1) sampler
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // @binding(2) PostGlobals uniform
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
        ];

        if params.byte_len() > 0 {
            // @binding(3) per-shader params uniform
            layout_entries.push(wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            });
        }

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}-bgl")),
            entries: &layout_entries,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{label}-layout")),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{label}-pipeline")),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[], // full-screen triangle from vertex_index
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_format,
                    blend: None, // post-processing: overwrite entirely
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

        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}-globals")),
            contents: bytemuck::bytes_of(&PostGlobals::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let params_buffer = if params.byte_len() > 0 {
            Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("{label}-params")),
                    contents: params.as_bytes(),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                }),
            )
        } else {
            None
        };

        Ok(Self {
            label: label.to_owned(),
            pipeline,
            bind_group_layout,
            globals_buffer,
            params_buffer,
            params,
        })
    }

    /// Update the parameter buffer on the GPU (call when manifest params change).
    pub fn update_params(&mut self, queue: &wgpu::Queue, params: ShaderParams) {
        self.params = params;
        if let Some(buf) = &self.params_buffer {
            let bytes = self.params.as_bytes();
            if !bytes.is_empty() {
                queue.write_buffer(buf, 0, bytes);
            }
        }
    }

    /// Create a bind group targeting a specific input texture.
    fn create_bind_group(
        &self,
        device: &wgpu::Device,
        input_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(input_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: self.globals_buffer.as_entire_binding(),
            },
        ];
        if let Some(buf) = &self.params_buffer {
            entries.push(wgpu::BindGroupEntry {
                binding: 3,
                resource: buf.as_entire_binding(),
            });
        }

        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{}-bg", self.label)),
            layout: &self.bind_group_layout,
            entries: &entries,
        })
    }

    /// Execute this shader pass: reads `input_view`, writes to `output_view`.
    fn execute(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        globals: &PostGlobals,
    ) {
        queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(globals));

        let bind_group = self.create_bind_group(device, input_view, sampler);

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(&format!("{}-pass", self.label)),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
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
        pass.draw(0..3, 0..1); // full-screen triangle
    }
}

// ---------------------------------------------------------------------------
// PostStack — ordered list of shader passes with ping-pong textures
// ---------------------------------------------------------------------------

/// Ordered stack of post-processing passes. Applied sequentially using
/// ping-pong intermediate textures so each pass reads the output of the previous.
pub struct PostStack {
    passes: Vec<ShaderPass>,
    /// Ping-pong textures for intermediate results. Only allocated when passes > 0.
    ping_texture: Option<wgpu::Texture>,
    pong_texture: Option<wgpu::Texture>,
    sampler: wgpu::Sampler,
    width: u32,
    height: u32,
}

impl PostStack {
    /// Create a new (possibly empty) post-processing stack.
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        render_format: wgpu::TextureFormat,
        effects: &[PostEffect],
    ) -> Result<Self> {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        if effects.is_empty() {
            return Ok(Self {
                passes: Vec::new(),
                ping_texture: None,
                pong_texture: None,
                sampler,
                width,
                height,
            });
        }

        let mut passes = Vec::with_capacity(effects.len());
        for effect in effects {
            let pass = build_shader_pass(device, effect, render_format)?;
            passes.push(pass);
        }

        let make_tex = |label: &str| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
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
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };

        let ping_texture = make_tex("post-ping");
        let pong_texture = make_tex("post-pong");

        Ok(Self {
            passes,
            ping_texture: Some(ping_texture),
            pong_texture: Some(pong_texture),
            sampler,
            width,
            height,
        })
    }

    /// Returns true if there are no post-processing passes to apply.
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// Apply all passes sequentially. Reads from `source_texture` and writes
    /// the final result back into `source_texture` via a copy.
    ///
    /// The flow:
    ///   1. Copy source → ping
    ///   2. For each pass: render ping → pong, then swap ping/pong
    ///   3. Copy final result (which is in ping after last swap) → source
    pub fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source_texture: &wgpu::Texture,
        frame_index: u32,
        fps: u32,
        seed: u64,
    ) {
        if self.passes.is_empty() {
            return;
        }

        let ping = self.ping_texture.as_ref().expect("ping allocated");
        let pong = self.pong_texture.as_ref().expect("pong allocated");

        let globals = PostGlobals {
            resolution: [self.width as f32, self.height as f32],
            time: frame_index as f32 / fps as f32,
            frame_index,
            seed: seed as u32,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };

        let extent = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };

        // Step 1: copy source → ping
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: source_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
                texture: ping,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            extent,
        );

        // Step 2: for each pass, read from `current_input`, write to `current_output`, then swap.
        let mut read_from_ping = true;
        for pass in &self.passes {
            let (input_tex, output_tex) = if read_from_ping {
                (ping, pong)
            } else {
                (pong, ping)
            };
            let input_view = input_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let output_view = output_tex.create_view(&wgpu::TextureViewDescriptor::default());

            pass.execute(
                device,
                queue,
                encoder,
                &input_view,
                &output_view,
                &self.sampler,
                &globals,
            );

            read_from_ping = !read_from_ping;
        }

        // Step 3: copy result → source
        let result_tex = if read_from_ping { ping } else { pong };
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: result_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
                texture: source_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            extent,
        );
    }
}

// ---------------------------------------------------------------------------
// Builder: PostEffect → ShaderPass
// ---------------------------------------------------------------------------

fn build_shader_pass(
    device: &wgpu::Device,
    effect: &PostEffect,
    render_format: wgpu::TextureFormat,
) -> Result<ShaderPass> {
    match &effect.kind {
        PostEffectKind::Passthrough => ShaderPass::new(
            device,
            "post-passthrough",
            PASSTHROUGH_WGSL,
            ShaderParams::None,
            render_format,
        ),
        PostEffectKind::Levels { gamma, lift, gain } => {
            let params = LevelsParams {
                gamma: *gamma,
                lift: *lift,
                gain: *gain,
                _pad: 0.0,
            };
            ShaderPass::new(
                device,
                "post-levels",
                LEVELS_WGSL,
                ShaderParams::Levels(params),
                render_format,
            )
        }
        PostEffectKind::Sobel { strength } => {
            let params = SobelParams {
                strength: *strength,
                _pad0: 0.0,
                _pad1: 0.0,
                _pad2: 0.0,
            };
            ShaderPass::new(
                device,
                "post-sobel",
                SOBEL_WGSL,
                ShaderParams::Sobel(params),
                render_format,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_globals_is_32_bytes_and_16_aligned() {
        assert_eq!(
            std::mem::size_of::<PostGlobals>(),
            32,
            "PostGlobals must be 32 bytes to match WGSL struct"
        );
        assert_eq!(
            std::mem::align_of::<PostGlobals>(),
            4, // bytemuck Pod aligns to field max (u32/f32 = 4)
            "PostGlobals alignment"
        );
    }

    #[test]
    fn levels_params_is_16_bytes() {
        assert_eq!(
            std::mem::size_of::<LevelsParams>(),
            16,
            "LevelsParams must be 16 bytes (vec4-sized) to match WGSL struct"
        );
    }

    #[test]
    fn sobel_params_is_16_bytes() {
        assert_eq!(
            std::mem::size_of::<SobelParams>(),
            16,
            "SobelParams must be 16 bytes (vec4-sized) to match WGSL struct"
        );
    }

    #[test]
    fn levels_defaults_are_identity() {
        let p = LevelsParams::default();
        assert!((p.gamma - 1.0).abs() < f32::EPSILON);
        assert!((p.lift - 0.0).abs() < f32::EPSILON);
        assert!((p.gain - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn sobel_default_strength_is_one() {
        let p = SobelParams::default();
        assert!((p.strength - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn shader_params_none_has_zero_length() {
        assert_eq!(ShaderParams::None.byte_len(), 0);
    }

    #[test]
    fn shader_params_levels_has_correct_length() {
        let p = ShaderParams::Levels(LevelsParams::default());
        assert_eq!(p.byte_len(), 16);
    }

    #[test]
    fn shader_params_sobel_has_correct_length() {
        let p = ShaderParams::Sobel(SobelParams::default());
        assert_eq!(p.byte_len(), 16);
    }

    #[test]
    fn embedded_wgsl_sources_are_nonempty() {
        assert!(!PASSTHROUGH_WGSL.is_empty());
        assert!(!LEVELS_WGSL.is_empty());
        assert!(!SOBEL_WGSL.is_empty());
    }

    #[test]
    fn wgsl_sources_contain_expected_entry_points() {
        assert!(PASSTHROUGH_WGSL.contains("fn vs_main"));
        assert!(PASSTHROUGH_WGSL.contains("fn fs_main"));
        assert!(LEVELS_WGSL.contains("fn vs_main"));
        assert!(LEVELS_WGSL.contains("fn fs_main"));
        assert!(SOBEL_WGSL.contains("fn vs_main"));
        assert!(SOBEL_WGSL.contains("fn fs_main"));
    }

    #[test]
    fn wgsl_sources_declare_post_globals() {
        assert!(PASSTHROUGH_WGSL.contains("struct PostGlobals"));
        assert!(LEVELS_WGSL.contains("struct PostGlobals"));
        assert!(SOBEL_WGSL.contains("struct PostGlobals"));
    }

    #[test]
    fn levels_wgsl_declares_params_struct() {
        assert!(LEVELS_WGSL.contains("struct LevelsParams"));
        assert!(LEVELS_WGSL.contains("gamma: f32"));
        assert!(LEVELS_WGSL.contains("lift: f32"));
        assert!(LEVELS_WGSL.contains("gain: f32"));
    }

    #[test]
    fn sobel_wgsl_declares_params_struct() {
        assert!(SOBEL_WGSL.contains("struct SobelParams"));
        assert!(SOBEL_WGSL.contains("strength: f32"));
    }
}
