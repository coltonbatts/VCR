use serde::Serialize;

/// Error types for agent-readable error reporting
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorType {
    Validation,
    Build,
    Lint,
    Io,
    MissingDependency,
    Usage,
}

/// Structured error report for AI agents to parse and auto-correct
#[derive(Debug, Clone, Serialize)]
pub struct AgentErrorReport {
    pub error_type: AgentErrorType,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<SuggestedFix>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ErrorContext>,
}

/// Context about where the error occurred
#[derive(Debug, Clone, Serialize)]
pub struct ErrorContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// Suggested fix that an agent can apply programmatically
#[derive(Debug, Clone, Serialize)]
pub struct SuggestedFix {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yaml_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<FixAction>>,
}

/// Specific actions an agent can take to fix the error
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FixAction {
    AddField {
        path: String,
        field_name: String,
        example_value: String,
    },
    ReplaceValue {
        path: String,
        current: String,
        suggested: String,
    },
    RemoveField {
        path: String,
        field_name: String,
    },
    SetParameter {
        param_name: String,
        expected_type: String,
        example_value: String,
    },
}

impl AgentErrorReport {
    /// Create a validation error report
    pub fn validation(summary: impl Into<String>) -> Self {
        Self {
            error_type: AgentErrorType::Validation,
            summary: summary.into(),
            suggested_fix: None,
            context: None,
        }
    }

    /// Create a lint error report
    pub fn lint(summary: impl Into<String>) -> Self {
        Self {
            error_type: AgentErrorType::Lint,
            summary: summary.into(),
            suggested_fix: None,
            context: None,
        }
    }

    /// Create a build error report
    pub fn build(summary: impl Into<String>) -> Self {
        Self {
            error_type: AgentErrorType::Build,
            summary: summary.into(),
            suggested_fix: None,
            context: None,
        }
    }

    /// Add a suggested fix to the error report
    pub fn with_fix(mut self, fix: SuggestedFix) -> Self {
        self.suggested_fix = Some(fix);
        self
    }

    /// Add context to the error report
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        self.context = Some(context);
        self
    }

    /// Convert to JSON string for agent consumption
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

impl SuggestedFix {
    /// Create a simple fix with just a description
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            yaml_snippet: None,
            actions: None,
        }
    }

    /// Add a YAML snippet showing the correct syntax
    pub fn with_yaml(mut self, yaml: impl Into<String>) -> Self {
        self.yaml_snippet = Some(yaml.into());
        self
    }

    /// Add programmatic actions
    pub fn with_actions(mut self, actions: Vec<FixAction>) -> Self {
        self.actions = Some(actions);
        self
    }
}

impl ErrorContext {
    pub fn new() -> Self {
        Self {
            file: None,
            line: None,
            field: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_line(mut self, line: u32) -> Self {
        self.line = Some(line);
        self
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }
}

/// Helper to detect common validation error patterns and provide fixes
pub fn suggest_fix_for_validation_error(error_message: &str) -> Option<SuggestedFix> {
    // Missing field errors
    if error_message.contains("missing field") {
        if error_message.contains("duration") {
            return Some(
                SuggestedFix::new("Add the 'duration' field to the environment block")
                    .with_yaml("environment:\n  duration: 3.0  # or { frames: 90 }")
                    .with_actions(vec![FixAction::AddField {
                        path: "environment".to_string(),
                        field_name: "duration".to_string(),
                        example_value: "3.0".to_string(),
                    }]),
            );
        }
        if error_message.contains("'fps'") {
            return Some(
                SuggestedFix::new("Add the 'fps' field to the environment block")
                    .with_yaml("environment:\n  fps: 30")
                    .with_actions(vec![FixAction::AddField {
                        path: "environment".to_string(),
                        field_name: "fps".to_string(),
                        example_value: "30".to_string(),
                    }]),
            );
        }
        if error_message.contains("'resolution'") {
            return Some(
                SuggestedFix::new("Add the 'resolution' field to the environment block")
                    .with_yaml("environment:\n  resolution: { width: 1920, height: 1080 }")
                    .with_actions(vec![FixAction::AddField {
                        path: "environment".to_string(),
                        field_name: "resolution".to_string(),
                        example_value: "{ width: 1920, height: 1080 }".to_string(),
                    }]),
            );
        }
    }

    // Unknown variant errors
    if error_message.contains("unknown variant") && error_message.contains("solid_colour") {
        return Some(
            SuggestedFix::new("Use 'solid_color' instead of 'solid_colour' (American spelling)")
                .with_yaml("procedural:\n  kind: solid_color")
                .with_actions(vec![FixAction::ReplaceValue {
                    path: "layers[].procedural.kind".to_string(),
                    current: "solid_colour".to_string(),
                    suggested: "solid_color".to_string(),
                }]),
        );
    }

    // Type mismatch errors
    if error_message.contains("expected") && error_message.contains("got") {
        if error_message.contains("float") {
            return Some(
                SuggestedFix::new("Value must be a number (integer or float)")
                    .with_yaml("# Example: opacity: 0.75"),
            );
        }
    }

    // Layer must have exactly one source
    if error_message.contains("must have exactly one source") {
        return Some(
            SuggestedFix::new("Each layer needs exactly one of: procedural, text, image, shader, or ascii")
                .with_yaml("layers:\n  - id: example\n    procedural:\n      kind: solid_color\n      color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }")
        );
    }

    None
}

/// Helper to suggest fixes for lint errors
pub fn suggest_fix_for_lint_error(layer_id: &str, issue: &str) -> Option<SuggestedFix> {
    if issue.contains("unreachable") || issue.contains("never visible") {
        return Some(
            SuggestedFix::new(format!(
                "Layer '{}' is never visible. Add timing constraints or adjust opacity.",
                layer_id
            ))
            .with_yaml(format!(
                "# Option 1: Set when layer appears\nstart_time: 0.0\nend_time: 3.0\n\n# Option 2: Ensure opacity is > 0\nopacity: 1.0\n\n# Option 3: Check z_index ordering\nz_index: 1"
            )),
        );
    }

    None
}

/// Helper to suggest fixes for parameter override errors
pub fn suggest_fix_for_param_error(
    param_name: &str,
    expected_type: &str,
    provided_value: &str,
) -> SuggestedFix {
    let example = match expected_type {
        "float" => "1.5",
        "int" => "10",
        "bool" => "true",
        "vec2" => "100,-50",
        "color" => "#ff0066",
        _ => "value",
    };

    SuggestedFix::new(format!(
        "Parameter '{}' expects type '{}', but got '{}'",
        param_name, expected_type, provided_value
    ))
    .with_yaml(format!("--set {}={}", param_name, example))
    .with_actions(vec![FixAction::SetParameter {
        param_name: param_name.to_string(),
        expected_type: expected_type.to_string(),
        example_value: example.to_string(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_error_report_json() {
        let report = AgentErrorReport::validation("Missing required field 'duration'")
            .with_fix(
                SuggestedFix::new("Add duration to environment")
                    .with_yaml("environment:\n  duration: 3.0"),
            )
            .with_context(ErrorContext::new().with_file("test.vcr").with_line(5));

        let json = report.to_json().unwrap();
        assert!(json.contains("\"error_type\": \"validation\""));
        assert!(json.contains("\"summary\""));
        assert!(json.contains("\"suggested_fix\""));
    }

    #[test]
    fn test_suggest_fix_for_missing_duration() {
        let fix = suggest_fix_for_validation_error("missing field 'duration'");
        assert!(fix.is_some());
        let fix = fix.unwrap();
        assert!(fix.yaml_snippet.is_some());
        assert!(fix.actions.is_some());
    }

    #[test]
    fn test_suggest_fix_for_typo() {
        let fix = suggest_fix_for_validation_error("unknown variant 'solid_colour'");
        assert!(fix.is_some());
        let fix = fix.unwrap();
        assert!(fix.description.contains("solid_color"));
    }
}
