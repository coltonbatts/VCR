use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use rapier3d::prelude::*;
use vcr::encoding::FfmpegPipe;
use vcr::renderer::Renderer;
use vcr::schema::{
    AnimatableColor, Duration as ManifestDuration, Environment, Layer, LayerCommon,
    ProceduralLayer, ProceduralSource, Resolution, ScalarProperty, Vec2,
};
use vcr::timeline::RenderSceneData;

fn main() -> Result<()> {
    let frame_count = 120;
    let fps = 24;
    let environment = Environment {
        resolution: Resolution {
            width: 640,
            height: 360,
        },
        fps,
        duration: ManifestDuration::Frames {
            frames: frame_count,
        },
        color_space: Default::default(),
    };

    println!("🚀 Starting Native Physics Handshake (Rapier3D)...");

    // --- Physics Setup ---
    let mut rigid_body_set = RigidBodySet::new();
    let mut collider_set = ColliderSet::new();

    let gravity = vector![0.0, -9.81, 0.0];
    let integration_parameters = IntegrationParameters::default();
    let mut physics_pipeline = PhysicsPipeline::new();
    let mut island_manager = IslandManager::new();
    let mut broad_phase = DefaultBroadPhase::new();
    let mut narrow_phase = NarrowPhase::new();
    let mut impulse_joint_set = ImpulseJointSet::new();
    let mut multibody_joint_set = MultibodyJointSet::new();
    let mut ccd_solver = CCDSolver::new();
    let physics_hooks = ();
    let event_handler = ();

    // Create Ground
    let ground_collider = ColliderBuilder::cuboid(100.0, 0.1, 100.0).build();
    collider_set.insert(ground_collider);

    // Create Cube
    let rigid_body = RigidBodyBuilder::dynamic()
        .translation(vector![0.0, 10.0, 0.0])
        .build();
    let handle = rigid_body_set.insert(rigid_body);
    let collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5)
        .restitution(0.7)
        .build();
    collider_set.insert_with_parent(collider, handle, &mut rigid_body_set);

    // --- Rendering Setup ---
    let output_mov = PathBuf::from("renders/native_physics_proof.mov");
    if let Some(parent) = output_mov.parent() {
        fs::create_dir_all(parent).ok();
    }

    // Define a Procedural Circle layer that we'll drive with physics
    let py_expr: ScalarProperty = serde_json::from_str("\"180 - (cube_y * 15)\"")?;

    let cube_layer = Layer::Procedural(ProceduralLayer {
        common: LayerCommon {
            id: "cube".to_string(),
            name: Some("Physics Cube".to_string()),
            position: vcr::schema::PropertyValue::Static(Vec2 { x: 320.0, y: 180.0 }),
            pos_y: Some(py_expr),
            ..Default::default()
        },
        procedural: ProceduralSource::Circle {
            center: Vec2 { x: 0.0, y: 0.0 },
            radius: ScalarProperty::Static(20.0),
            color: AnimatableColor {
                r: ScalarProperty::Static(1.0),
                g: ScalarProperty::Static(1.0),
                b: ScalarProperty::Static(1.0),
                a: ScalarProperty::Static(1.0),
            },
        },
    });

    let scene = RenderSceneData::default();
    let mut renderer = Renderer::new_software(&environment, &[cube_layer], scene)?;
    let ffmpeg = FfmpegPipe::spawn(&environment, &output_mov)?;

    println!(
        "📹 Recording {} frames to {}...",
        frame_count,
        output_mov.display()
    );

    for frame_index in 0..frame_count {
        // Step physics
        physics_pipeline.step(
            &gravity,
            &integration_parameters,
            &mut island_manager,
            &mut broad_phase,
            &mut narrow_phase,
            &mut rigid_body_set,
            &mut collider_set,
            &mut impulse_joint_set,
            &mut multibody_joint_set,
            &mut ccd_solver,
            None,
            &physics_hooks,
            &event_handler,
        );

        let body = &rigid_body_set[handle];
        let pos = body.translation();

        // Update parameters
        let mut params = BTreeMap::new();
        params.insert("cube_y".to_string(), pos.y);
        renderer.set_params(params);

        if frame_index % 24 == 0 {
            println!("Frame {}: Cube Y = {:.2}", frame_index, pos.y);
        }

        let rgba = renderer.render_frame_rgba(frame_index)?;
        ffmpeg.write_frame(rgba)?;
    }

    ffmpeg.finish()?;
    println!("✅ Done! Render saved to {}", output_mov.display());

    Ok(())
}
