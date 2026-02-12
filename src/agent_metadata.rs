use serde::Serialize;
use std::collections::BTreeMap;

use crate::schema::ParamValue;

/// Agent-readable metadata about a rendered frame or video
#[derive(Debug, Clone, Serialize)]
pub struct AgentContextMetadata {
    /// Summary of all layers that were rendered
    pub layers_rendered: Vec<LayerSummary>,
    /// Hash of the resolved manifest (for determinism tracking)
    pub manifest_hash: String,
    /// Final resolved parameters used in rendering
    pub resolved_params: BTreeMap<String, ParamValue>,
    /// Description of what was created (for iteration context)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Summary of a single layer's state for agent consumption
#[derive(Debug, Clone, Serialize)]
pub struct LayerSummary {
    /// Layer ID
    pub id: String,
    /// Layer type (e.g., "procedural:circle", "text", "image", "shader")
    pub layer_type: String,
    /// Final position at key frame (typically last frame)
    pub final_position: (f32, f32),
    /// Final opacity at key frame
    pub final_opacity: f32,
    /// Final scale at key frame
    pub final_scale: (f32, f32),
    /// Final rotation in degrees at key frame
    pub final_rotation_degrees: f32,
    /// Was this layer visible in the final frame?
    pub was_visible: bool,
    /// Computed expression values for animated properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression_values: Option<BTreeMap<String, f32>>,
}

impl AgentContextMetadata {
    /// Create agent context from evaluated layer debug states
    pub fn from_layer_states(
        layer_states: &[crate::timeline::LayerDebugState],
        manifest_hash: String,
        resolved_params: BTreeMap<String, ParamValue>,
    ) -> Self {
        let layers_rendered = layer_states
            .iter()
            .map(|layer_state| {
                // Determine layer type from the stable ID
                let layer_type = layer_state
                    .stable_id
                    .as_deref()
                    .map(classify_layer_type)
                    .unwrap_or_else(|| "unknown".to_string());

                LayerSummary {
                    id: layer_state.id.clone(),
                    layer_type,
                    final_position: (layer_state.position.x, layer_state.position.y),
                    final_opacity: layer_state.opacity,
                    final_scale: (layer_state.scale.x, layer_state.scale.y),
                    final_rotation_degrees: layer_state.rotation_degrees,
                    was_visible: layer_state.visible && layer_state.opacity > 0.0,
                    expression_values: None, // TODO: Track expression values during evaluation
                }
            })
            .collect();

        Self {
            layers_rendered,
            manifest_hash,
            resolved_params,
            description: None,
        }
    }

    /// Add a human-readable description of what was rendered
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
}

/// Classify layer type from stable ID for agent readability
fn classify_layer_type(stable_id: &str) -> String {
    // The stable_id typically contains the layer type
    // For example: "procedural:solid_color", "text", "image", "shader"
    if stable_id.contains("procedural") {
        if stable_id.contains("solid_color") {
            "procedural:solid_color".to_string()
        } else if stable_id.contains("circle") {
            "procedural:circle".to_string()
        } else if stable_id.contains("rounded_rect") {
            "procedural:rounded_rect".to_string()
        } else if stable_id.contains("gradient") {
            "procedural:gradient".to_string()
        } else if stable_id.contains("ring") {
            "procedural:ring".to_string()
        } else if stable_id.contains("line") {
            "procedural:line".to_string()
        } else if stable_id.contains("triangle") {
            "procedural:triangle".to_string()
        } else if stable_id.contains("polygon") {
            "procedural:polygon".to_string()
        } else {
            "procedural".to_string()
        }
    } else if stable_id.contains("text") {
        "text".to_string()
    } else if stable_id.contains("image") {
        "image".to_string()
    } else if stable_id.contains("shader") {
        "shader".to_string()
    } else if stable_id.contains("ascii") {
        "ascii".to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_layer_type() {
        assert_eq!(
            classify_layer_type("procedural:solid_color"),
            "procedural:solid_color"
        );
        assert_eq!(
            classify_layer_type("procedural:circle"),
            "procedural:circle"
        );
        assert_eq!(classify_layer_type("text"), "text");
        assert_eq!(classify_layer_type("image"), "image");
        assert_eq!(classify_layer_type("shader"), "shader");
    }

    #[test]
    fn test_agent_context_serialization() {
        let mut params = BTreeMap::new();
        params.insert("speed".to_string(), ParamValue::Float(1.5));

        let context = AgentContextMetadata {
            layers_rendered: vec![LayerSummary {
                id: "bg".to_string(),
                layer_type: "procedural:solid_color".to_string(),
                final_position: (0.0, 0.0),
                final_opacity: 1.0,
                final_scale: (1.0, 1.0),
                final_rotation_degrees: 0.0,
                was_visible: true,
                expression_values: None,
            }],
            manifest_hash: "abc123".to_string(),
            resolved_params: params,
            description: Some("Simple background".to_string()),
        };

        let json = serde_json::to_string_pretty(&context).unwrap();
        assert!(json.contains("\"layers_rendered\""));
        assert!(json.contains("\"manifest_hash\""));
        assert!(json.contains("\"resolved_params\""));
        assert!(json.contains("\"description\""));
    }
}
