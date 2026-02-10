use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, bail, Context, Result};

use crate::schema::{
    ExpressionContext, Group, LayerCommon, Manifest, ModulatorBinding, ModulatorMap, Parameters,
    PropertyValue, ScalarProperty, TimingControls, Vec2,
};

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

#[derive(Debug, Clone, Copy)]
pub(crate) struct EvaluatedLayerState {
    pub position: Vec2,
    pub scale: Vec2,
    pub rotation_degrees: f32,
    pub opacity: f32,
}

pub(crate) fn resolve_groups_by_id(groups: &[Group]) -> BTreeMap<String, Group> {
    groups
        .iter()
        .cloned()
        .map(|group| (group.id.clone(), group))
        .collect::<BTreeMap<_, _>>()
}

pub fn evaluate_manifest_layers_at_frame(
    manifest: &Manifest,
    frame_index: u32,
) -> Result<Vec<LayerDebugState>> {
    let scene = RenderSceneData::from_manifest(manifest);
    let groups_by_id = resolve_groups_by_id(&scene.groups);

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

pub(crate) fn resolve_group_chain(
    common: &LayerCommon,
    groups_by_id: &BTreeMap<String, Group>,
) -> Result<Vec<Group>> {
    let Some(group_id) = common.group.as_deref() else {
        return Ok(Vec::new());
    };

    let mut chain = Vec::new();
    let mut seen = HashSet::new();
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
pub(crate) fn evaluate_layer_state(
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
pub(crate) fn evaluate_layer_state_or_hidden(
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
) -> Result<EvaluatedLayerState> {
    Ok(evaluate_layer_state(
        layer_id,
        position,
        position_x,
        position_y,
        scale,
        rotation_degrees,
        opacity,
        timing,
        layer_modulators,
        group_chain,
        frame_index,
        fps,
        params,
        seed,
        modulator_defs,
    )?
    .unwrap_or(EvaluatedLayerState {
        position: Vec2 { x: 0.0, y: 0.0 },
        scale: Vec2 { x: 1.0, y: 1.0 },
        rotation_degrees: 0.0,
        opacity: 0.0,
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

#[cfg(test)]
mod tests {
    use super::{
        evaluate_layer_state_or_hidden, evaluate_manifest_layers_at_frame, resolve_group_chain,
    };
    use crate::schema::Manifest;

    #[test]
    fn evaluate_manifest_layers_sorted_by_z_index() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 12 }
layers:
  - id: top
    z_index: 5
    procedural:
      kind: solid_color
      color: { r: 1, g: 0, b: 0, a: 1 }
  - id: bottom
    z_index: -2
    procedural:
      kind: solid_color
      color: { r: 0, g: 0, b: 0, a: 1 }
"#,
        )
        .expect("manifest should parse");

        let states =
            evaluate_manifest_layers_at_frame(&manifest, 0).expect("evaluation should succeed");
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].id, "bottom");
        assert_eq!(states[1].id, "top");
    }

    #[test]
    fn evaluate_manifest_layer_respects_start_time_window() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 48 }
layers:
  - id: delayed
    start_time: 1.0
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
        )
        .expect("manifest should parse");

        let early =
            evaluate_manifest_layers_at_frame(&manifest, 0).expect("evaluation should succeed");
        assert!(!early[0].visible);

        let visible =
            evaluate_manifest_layers_at_frame(&manifest, 24).expect("evaluation should succeed");
        assert!(visible[0].visible);
    }

    #[test]
    fn evaluate_layer_state_or_hidden_returns_zero_opacity_for_inactive_frame() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 48 }
layers:
  - id: delayed
    start_time: 2.0
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
        )
        .expect("manifest should parse");

        let layer = manifest.layers.first().expect("layer should exist");
        let common = layer.common();
        let groups_by_id = std::collections::BTreeMap::new();
        let group_chain =
            resolve_group_chain(common, &groups_by_id).expect("group chain should resolve");

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
            manifest.environment.fps,
            &manifest.params,
            manifest.seed,
            &manifest.modulators,
        )
        .expect("state evaluation should succeed");

        assert_eq!(state.opacity, 0.0);
    }
}
