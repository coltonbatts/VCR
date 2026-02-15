#![cfg(feature = "play")]
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, RichText};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Event as WinitEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

use crate::font_assets::read_verified_font_bytes;
use crate::manifest::load_and_validate_manifest;
use crate::renderer::{Renderer, RendererGpuContext};
use crate::schema::Manifest;
use crate::timeline::RenderSceneData;

#[derive(Debug, Clone, Copy)]
pub struct PlayArgs {
    pub start_frame: u32,
    pub paused: bool,
}

pub fn run_play(manifest_path: &Path, args: PlayArgs) -> Result<()> {
    let manifest_path = canonical_manifest_path(manifest_path);
    let mut manifest = load_and_validate_manifest(&manifest_path)?;
    let mut fps = manifest.environment.fps;
    let mut total_frames = manifest.environment.total_frames();
    if args.start_frame >= total_frames {
        return Err(anyhow!(
            "--start-frame {} is out of bounds for {} frame(s)",
            args.start_frame,
            total_frames
        ));
    }

    let event_loop = EventLoop::new().context("failed to create play event loop")?;
    let initial_size = PhysicalSize::new(
        manifest.environment.resolution.width,
        manifest.environment.resolution.height,
    );
    let window = Arc::new(
        WindowBuilder::new()
            .with_title(format!("VCR Play - {}", manifest_path.display()))
            .with_inner_size(initial_size)
            .with_resizable(false)
            .build(&event_loop)
            .context("failed to create preview window")?,
    );

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let surface = instance
        .create_surface(window.clone())
        .context("failed to create wgpu surface")?;
    let gpu_context = pollster::block_on(RendererGpuContext::for_surface(instance, &surface))
        .with_context(|| {
            format!(
                "failed to initialize WGPU context for {}",
                manifest_path.display()
            )
        })?;

    let caps = surface.get_capabilities(&gpu_context.adapter);
    let format = pick_surface_format(&caps.formats);
    let mut renderer = build_gpu_renderer(&manifest, &gpu_context, format)?;
    let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };
    let alpha_mode = caps
        .alpha_modes
        .first()
        .copied()
        .unwrap_or(wgpu::CompositeAlphaMode::Auto);

    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: initial_size.width.max(1),
        height: initial_size.height.max(1),
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&gpu_context.device, &surface_config);

    let mut playback = PlaybackClock::new(args.start_frame, args.paused, total_frames);
    let mut next_redraw_at = Instant::now();

    let (watch_tx, watch_rx) = mpsc::channel::<()>();
    let watched_manifest = manifest_path.to_path_buf();
    let watcher_manifest = watched_manifest.clone();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<Event>| match result {
            Ok(event) => {
                if should_reload(&event) && event_targets_manifest(&event, &watcher_manifest) {
                    let _ = watch_tx.send(());
                }
            }
            Err(error) => {
                eprintln!("[VCR] play: file watcher error: {error}");
            }
        })
        .context("failed to create file watcher")?;
    let watch_root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    watcher
        .watch(&watch_root, RecursiveMode::NonRecursive)
        .with_context(|| format!("failed to watch {}", watch_root.display()))?;

    let egui_ctx = egui::Context::default();
    configure_egui_fonts(&egui_ctx);
    let viewport_id = egui::ViewportId::ROOT;
    let mut egui_state =
        egui_winit::State::new(egui_ctx.clone(), viewport_id, &event_loop, None, None);
    let mut egui_renderer = EguiRenderer::new(&gpu_context.device, surface_config.format, None, 1);

    eprintln!(
        "[VCR] play: {}x{} @ {}fps ({} frames)",
        manifest.environment.resolution.width,
        manifest.environment.resolution.height,
        fps,
        total_frames
    );
    eprintln!(
        "[VCR] Controls: Space play/pause, Left/Right seek, R restart, Esc quit, drag seek bar"
    );
    eprintln!(
        "[VCR] State: frame {} ({})",
        playback.current_frame,
        if playback.is_playing {
            "playing"
        } else {
            "paused"
        }
    );
    eprintln!(
        "[VCR] Backend: {} ({})",
        renderer.backend_name(),
        renderer.backend_reason()
    );

    let manifest_path = watched_manifest;
    event_loop
        .run(move |event, target| {
            target.set_control_flow(ControlFlow::Wait);

            match event {
                WinitEvent::WindowEvent { window_id, event } if window_id == window.id() => {
                    let egui_response = egui_state.on_window_event(&window, &event);
                    if egui_response.repaint {
                        window.request_redraw();
                    }
                    match event {
                        WindowEvent::CloseRequested => target.exit(),
                        WindowEvent::KeyboardInput { event, .. } => {
                            if event.state == ElementState::Pressed
                                && !event.repeat
                                && !egui_response.consumed
                            {
                                handle_keyboard_event(
                                    event.physical_key,
                                    &mut playback,
                                    fps,
                                    total_frames,
                                    target,
                                );
                                next_redraw_at = Instant::now();
                                window.request_redraw();
                            }
                        }
                        WindowEvent::RedrawRequested => {
                            playback.sync_with_clock(fps, total_frames);
                            render_frame(
                                &window,
                                &surface,
                                &gpu_context,
                                &mut surface_config,
                                &mut renderer,
                                &egui_ctx,
                                &mut egui_state,
                                &mut egui_renderer,
                                &mut playback,
                                fps,
                                total_frames,
                            );
                        }
                        WindowEvent::Resized(size) => {
                            if size.width > 0 && size.height > 0 {
                                surface_config.width = size.width;
                                surface_config.height = size.height;
                                surface.configure(&gpu_context.device, &surface_config);
                            }
                        }
                        _ => {}
                    }
                }
                WinitEvent::AboutToWait => {
                    let mut manifest_dirty = false;
                    while watch_rx.try_recv().is_ok() {
                        manifest_dirty = true;
                    }

                    if manifest_dirty {
                        try_hot_reload(
                            &manifest_path,
                            &gpu_context,
                            &window,
                            &surface,
                            &mut surface_config,
                            &mut manifest,
                            &mut renderer,
                            &mut fps,
                            &mut total_frames,
                            &mut playback,
                        );
                        next_redraw_at = Instant::now();
                        window.request_redraw();
                    }

                    if playback.is_playing {
                        let frame_duration = frame_interval(fps);
                        let now = Instant::now();
                        if now >= next_redraw_at {
                            window.request_redraw();
                            next_redraw_at = now + frame_duration;
                        }
                        target.set_control_flow(ControlFlow::WaitUntil(next_redraw_at));
                    }
                }
                _ => {}
            }
        })
        .map_err(|error| anyhow!("play event loop terminated: {error}"))
}

fn render_frame(
    window: &winit::window::Window,
    surface: &wgpu::Surface<'_>,
    gpu_context: &RendererGpuContext,
    surface_config: &mut wgpu::SurfaceConfiguration,
    renderer: &mut Renderer,
    egui_ctx: &egui::Context,
    egui_state: &mut egui_winit::State,
    egui_renderer: &mut EguiRenderer,
    playback: &mut PlaybackClock,
    fps: u32,
    total_frames: u32,
) {
    if surface_config.width == 0 || surface_config.height == 0 {
        return;
    }

    let frame = match surface.get_current_texture() {
        Ok(frame) => frame,
        Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
            surface.configure(&gpu_context.device, surface_config);
            return;
        }
        Err(wgpu::SurfaceError::Timeout) => {
            return;
        }
        Err(wgpu::SurfaceError::OutOfMemory) => {
            eprintln!("[VCR] play: surface out of memory");
            return;
        }
    };

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    if let Err(error) = renderer.render_frame_to_view(playback.current_frame, &view) {
        eprintln!("[VCR] play: render error: {error:#}");
        frame.present();
        return;
    }

    let raw_input = egui_state.take_egui_input(window);
    let mut seek_target = None;
    let full_output = egui_ctx.run(raw_input, |ctx| {
        draw_overlay(ctx, playback, fps, total_frames, &mut seek_target);
    });

    egui_state.handle_platform_output(window, full_output.platform_output);
    let pixels_per_point = window.scale_factor() as f32;
    let paint_jobs = egui_ctx.tessellate(full_output.shapes, pixels_per_point);

    for (texture_id, delta) in &full_output.textures_delta.set {
        egui_renderer.update_texture(&gpu_context.device, &gpu_context.queue, *texture_id, delta);
    }

    let mut encoder = gpu_context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("vcr-egui-overlay"),
        });

    let screen_descriptor = ScreenDescriptor {
        size_in_pixels: [surface_config.width, surface_config.height],
        pixels_per_point,
    };
    egui_renderer.update_buffers(
        &gpu_context.device,
        &gpu_context.queue,
        &mut encoder,
        &paint_jobs,
        &screen_descriptor,
    );

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vcr-egui-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        egui_renderer.render(&mut pass, &paint_jobs, &screen_descriptor);
    }

    for texture_id in &full_output.textures_delta.free {
        egui_renderer.free_texture(texture_id);
    }

    gpu_context.queue.submit(Some(encoder.finish()));

    if let Some(frame_target) = seek_target {
        playback.seek_to(frame_target, fps, total_frames);
        window.request_redraw();
    }

    frame.present();
}

fn draw_overlay(
    ctx: &egui::Context,
    playback: &PlaybackClock,
    fps: u32,
    total_frames: u32,
    seek_target: &mut Option<u32>,
) {
    let max_frame = total_frames.saturating_sub(1);
    let frame_text = format!("{:06} / {:06}", playback.current_frame, max_frame);
    let time_text = format!("t {:.3}s", playback.current_frame as f32 / fps as f32);

    egui::Area::new("frame_counter".into())
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(Color32::from_black_alpha(120))
                .rounding(egui::Rounding::same(3.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(frame_text)
                            .font(FontId::new(18.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(180, 255, 180)),
                    );
                    ui.label(
                        RichText::new(time_text)
                            .font(FontId::new(13.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(160, 220, 160)),
                    );
                });
        });

    egui::TopBottomPanel::bottom("seek_bar")
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_black_alpha(110))
                .inner_margin(egui::Margin::symmetric(12.0, 8.0)),
        )
        .show(ctx, |ui| {
            let mut slider_frame = playback.current_frame as f64;
            let response = ui.add(
                egui::Slider::new(&mut slider_frame, 0.0..=max_frame as f64)
                    .show_value(false)
                    .text(""),
            );

            if response.changed() {
                *seek_target = Some(slider_frame.round() as u32);
            }
        });
}

fn configure_egui_fonts(egui_ctx: &egui::Context) {
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    match read_verified_font_bytes(manifest_root, "GeistPixel-Line.ttf") {
        Ok(font_bytes) => {
            let mut fonts = FontDefinitions::default();
            fonts.font_data.insert(
                "geist_pixel".into(),
                FontData::from_owned(font_bytes).into(),
            );
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "geist_pixel".into());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "geist_pixel".into());
            egui_ctx.set_fonts(fonts);
        }
        Err(error) => {
            eprintln!(
                "[VCR] play: failed to load verified Geist Pixel font GeistPixel-Line.ttf: {error}"
            );
        }
    }
}

fn build_gpu_renderer(
    manifest: &Manifest,
    gpu_context: &RendererGpuContext,
    render_format: wgpu::TextureFormat,
) -> Result<Renderer> {
    let scene = RenderSceneData::from_manifest(manifest);
    let renderer = Renderer::new_with_scene_and_context(
        &manifest.environment,
        &manifest.layers,
        scene,
        gpu_context,
        render_format,
    )?;

    if renderer.is_gpu_backend() {
        Ok(renderer)
    } else {
        Err(anyhow!(
            "play currently requires GPU backend; software fallback is disabled for this command"
        ))
    }
}

fn try_hot_reload(
    manifest_path: &Path,
    gpu_context: &RendererGpuContext,
    window: &winit::window::Window,
    surface: &wgpu::Surface<'_>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    manifest: &mut Manifest,
    renderer: &mut Renderer,
    fps: &mut u32,
    total_frames: &mut u32,
    playback: &mut PlaybackClock,
) {
    let next_manifest = match load_and_validate_manifest(manifest_path) {
        Ok(next_manifest) => next_manifest,
        Err(error) => {
            eprintln!("[VCR] play: reload parse error: {error:#}");
            return;
        }
    };

    let next_renderer = match build_gpu_renderer(&next_manifest, gpu_context, surface_config.format)
    {
        Ok(next_renderer) => next_renderer,
        Err(error) => {
            eprintln!("[VCR] play: reload failed to rebuild renderer: {error:#}");
            return;
        }
    };

    if next_manifest.environment.resolution.width != manifest.environment.resolution.width
        || next_manifest.environment.resolution.height != manifest.environment.resolution.height
    {
        let new_size = PhysicalSize::new(
            next_manifest.environment.resolution.width,
            next_manifest.environment.resolution.height,
        );
        let _ = window.request_inner_size(new_size);
        surface_config.width = new_size.width.max(1);
        surface_config.height = new_size.height.max(1);
        surface.configure(&gpu_context.device, surface_config);
    }

    *fps = next_manifest.environment.fps;
    *total_frames = next_manifest.environment.total_frames();
    playback.reset_timeline(*total_frames);
    *renderer = next_renderer;
    *manifest = next_manifest;

    eprintln!(
        "[VCR] play: reloaded {} ({}x{}, {}fps, {} frames)",
        manifest_path.display(),
        manifest.environment.resolution.width,
        manifest.environment.resolution.height,
        *fps,
        *total_frames
    );
}

fn should_reload(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) | EventKind::Any
    )
}

fn event_targets_manifest(event: &Event, manifest_path: &Path) -> bool {
    if event.paths.is_empty() {
        return true;
    }

    event.paths.iter().any(|path| {
        path == manifest_path
            || std::fs::canonicalize(path)
                .map(|resolved| resolved == manifest_path)
                .unwrap_or(false)
    })
}

fn canonical_manifest_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn frame_interval(fps: u32) -> Duration {
    Duration::from_secs_f64(1.0 / fps as f64)
}

fn pick_surface_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    formats
        .iter()
        .copied()
        .find(|format| format.is_srgb())
        .unwrap_or_else(|| formats[0])
}

fn handle_keyboard_event(
    key: PhysicalKey,
    playback: &mut PlaybackClock,
    fps: u32,
    total_frames: u32,
    target: &winit::event_loop::EventLoopWindowTarget<()>,
) {
    match key {
        PhysicalKey::Code(KeyCode::Space) => playback.toggle_play_pause(fps, total_frames),
        PhysicalKey::Code(KeyCode::ArrowLeft) => playback.seek_relative(-1, fps, total_frames),
        PhysicalKey::Code(KeyCode::ArrowRight) => playback.seek_relative(1, fps, total_frames),
        PhysicalKey::Code(KeyCode::KeyR) => playback.restart(fps, total_frames),
        PhysicalKey::Code(KeyCode::Escape) => target.exit(),
        _ => {}
    }
}

struct PlaybackClock {
    current_frame: u32,
    is_playing: bool,
    playback_anchor: Instant,
    anchor_frame: u32,
}

impl PlaybackClock {
    fn new(start_frame: u32, paused: bool, total_frames: u32) -> Self {
        let now = Instant::now();
        let clamped_frame = if total_frames == 0 {
            0
        } else {
            start_frame.min(total_frames - 1)
        };
        Self {
            current_frame: clamped_frame,
            is_playing: !paused,
            playback_anchor: now,
            anchor_frame: clamped_frame,
        }
    }

    fn sync_with_clock(&mut self, fps: u32, total_frames: u32) {
        if !self.is_playing || total_frames == 0 {
            return;
        }

        let elapsed_frames = (self.playback_anchor.elapsed().as_secs_f64() * fps as f64) as u32;
        self.current_frame = (self.anchor_frame + elapsed_frames) % total_frames;
    }

    fn toggle_play_pause(&mut self, fps: u32, total_frames: u32) {
        if self.is_playing {
            self.sync_with_clock(fps, total_frames);
            self.is_playing = false;
            return;
        }

        self.is_playing = true;
        self.playback_anchor = Instant::now();
        self.anchor_frame = self.current_frame;
    }

    fn seek_relative(&mut self, delta: i32, fps: u32, total_frames: u32) {
        if total_frames == 0 {
            return;
        }

        self.sync_with_clock(fps, total_frames);
        let next =
            (self.current_frame as i64 + delta as i64).rem_euclid(total_frames as i64) as u32;
        self.current_frame = next;
        self.reanchor_if_playing();
    }

    fn seek_to(&mut self, frame: u32, _fps: u32, total_frames: u32) {
        if total_frames == 0 {
            return;
        }

        self.current_frame = frame.min(total_frames - 1);
        self.reanchor_if_playing();
    }

    fn restart(&mut self, _fps: u32, total_frames: u32) {
        if total_frames == 0 {
            return;
        }

        self.current_frame = 0;
        self.reanchor_if_playing();
    }

    fn reset_timeline(&mut self, total_frames: u32) {
        if total_frames == 0 {
            self.current_frame = 0;
            self.anchor_frame = 0;
            self.playback_anchor = Instant::now();
            return;
        }

        self.current_frame = self.current_frame.min(total_frames - 1);
        self.anchor_frame = self.current_frame;
        self.playback_anchor = Instant::now();
    }

    fn reanchor_if_playing(&mut self) {
        if self.is_playing {
            self.anchor_frame = self.current_frame;
            self.playback_anchor = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlaybackClock;
    use std::time::{Duration, Instant};

    #[test]
    fn paused_clock_does_not_advance() {
        let mut clock = PlaybackClock::new(5, true, 120);
        clock.playback_anchor = Instant::now() - Duration::from_secs(30);
        clock.sync_with_clock(60, 120);
        assert_eq!(clock.current_frame, 5);
    }

    #[test]
    fn playing_clock_advances_and_wraps() {
        let mut clock = PlaybackClock::new(8, false, 10);
        clock.playback_anchor = Instant::now() - Duration::from_secs(5);
        clock.sync_with_clock(1, 10);
        assert_eq!(clock.current_frame, 3);
    }

    #[test]
    fn seek_relative_wraps_backwards() {
        let mut clock = PlaybackClock::new(0, true, 10);
        clock.seek_relative(-1, 60, 10);
        assert_eq!(clock.current_frame, 9);
    }

    #[test]
    fn restart_resets_to_zero() {
        let mut clock = PlaybackClock::new(6, false, 10);
        clock.restart(60, 10);
        assert_eq!(clock.current_frame, 0);
    }
}
