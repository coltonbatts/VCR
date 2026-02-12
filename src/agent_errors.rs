use anyhow::Error;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorType {
    Validation,
    Build,
    Lint,
    Io,
    MissingDependency,
    Usage,
}

#[derive(Debug, Serialize)]
pub struct AgentErrorReport {
    pub error_type: AgentErrorType,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<SuggestedFix>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ErrorContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_errors: Option<Vec<ValidationErrorDetail>>,
}

#[derive(Debug, Serialize)]
pub struct ErrorContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ValidationErrorDetail {
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SuggestedFix {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yaml_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<FixAction>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FixAction {
    AddField {
        path: String,
        field_name: String,
        // example_value is always a JSON scalar/array/object, never a stringified JSON blob.
        example_value: Value,
    },
    SetField {
        path: String,
        expected_type: String,
        // example_value is always a JSON scalar/array/object, never a stringified JSON blob.
        example_value: Value,
    },
    RemoveField {
        path: String,
    },
}

impl SuggestedFix {
    fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            yaml_snippet: None,
            actions: None,
        }
    }

    fn with_yaml(mut self, yaml_snippet: impl Into<String>) -> Self {
        self.yaml_snippet = Some(yaml_snippet.into());
        self
    }

    fn with_actions(mut self, actions: Vec<FixAction>) -> Self {
        self.actions = Some(actions);
        self
    }
}

pub fn build_agent_error_report(
    error_type: AgentErrorType,
    head: &str,
    summary: &str,
    error: &Error,
) -> AgentErrorReport {
    let normalized_summary = normalize_summary(summary);
    let (line, column) = extract_yaml_location(error);
    let context = extract_manifest_path(head).map(|file| ErrorContext {
        file: Some(file),
        line,
        column,
    });

    let suggested_fix = match error_type {
        AgentErrorType::Validation => suggest_fix_for_validation_error(&normalized_summary),
        AgentErrorType::Usage => suggest_fix_for_usage_error(&normalized_summary),
        AgentErrorType::Build
        | AgentErrorType::Lint
        | AgentErrorType::Io
        | AgentErrorType::MissingDependency => None,
    };

    let validation_errors = if matches!(error_type, AgentErrorType::Validation) {
        Some(vec![ValidationErrorDetail {
            path: infer_validation_path(&normalized_summary),
            message: normalized_summary.clone(),
            line,
            column,
        }])
    } else {
        None
    };

    AgentErrorReport {
        error_type,
        summary: normalized_summary,
        suggested_fix,
        context,
        validation_errors,
    }
}

fn suggest_fix_for_validation_error(summary: &str) -> Option<SuggestedFix> {
    if summary.contains("missing field `duration`") || summary.contains("missing field 'duration'")
    {
        return Some(
            SuggestedFix::new("Add the required environment.duration field")
                .with_yaml("environment:\n  duration: 3.0")
                .with_actions(vec![FixAction::AddField {
                    path: "environment".to_owned(),
                    field_name: "duration".to_owned(),
                    example_value: json!(3.0),
                }]),
        );
    }

    None
}

fn suggest_fix_for_usage_error(summary: &str) -> Option<SuggestedFix> {
    let (param_name, expected_type, provided) = parse_usage_param_error(summary)?;
    let example_value = example_value_for_param_type(expected_type);
    Some(
        SuggestedFix::new(format!(
            "Parameter '{param_name}' expects {expected_type} but got '{provided}'"
        ))
        .with_yaml(format!(
            "--set {param_name}={}",
            example_value_for_cli_hint(expected_type)
        ))
        .with_actions(vec![FixAction::SetField {
            path: format!("params.{param_name}"),
            expected_type: expected_type.to_owned(),
            example_value,
        }]),
    )
}

fn example_value_for_param_type(expected_type: &str) -> Value {
    match expected_type {
        "float" => json!(1.5),
        "int" => json!(10),
        "bool" => json!(true),
        "vec2" => json!([100, -50]),
        "color" => json!("#ff0066"),
        _ => json!("value"),
    }
}

fn example_value_for_cli_hint(expected_type: &str) -> &'static str {
    match expected_type {
        "float" => "1.5",
        "int" => "10",
        "bool" => "true",
        "vec2" => "100,-50",
        "color" => "#ff0066",
        _ => "value",
    }
}

fn parse_usage_param_error(summary: &str) -> Option<(String, &str, String)> {
    let param_name = extract_between(summary, "invalid --set for param '", "'")?.to_owned();
    let expected_type = extract_between(summary, "expected ", ", got '")?;
    let provided = extract_between(summary, ", got '", "'")?.to_owned();
    Some((param_name, expected_type, provided))
}

fn normalize_summary(summary: &str) -> String {
    // Keep agent summaries stable across machines by removing manifest path prefixes when possible.
    // Other path-bearing summary patterns may be normalized in a future minor release.
    if let Some(rest) = summary.strip_prefix("failed to decode manifest ") {
        if let Some((_, detail)) = rest.split_once(". ") {
            return format!("failed to decode manifest. {detail}");
        }
        return "failed to decode manifest".to_owned();
    }
    summary.to_owned()
}

fn infer_validation_path(summary: &str) -> String {
    if let Some(path) = parse_param_validation_path(summary) {
        return path;
    }

    if let Some(field) = extract_quoted_token_after(summary, "missing field") {
        return qualify_manifest_field_path(summary, &field);
    }

    if let Some(field) = extract_quoted_token_after(summary, "unknown field") {
        return qualify_manifest_field_path(summary, &field);
    }

    "$".to_owned()
}

fn parse_param_validation_path(summary: &str) -> Option<String> {
    let param_name = extract_between(summary, "param '", "'")?;

    if let Some(field) = extract_between(summary, "must define '", "'") {
        return Some(format!("params.{param_name}.{field}"));
    }

    if let Some(field) = extract_between(summary, "has unknown field '", "'") {
        return Some(format!("params.{param_name}.{field}"));
    }

    let marker = format!("param '{param_name}'");
    let after = summary.split(&marker).nth(1)?;
    let remainder = after.strip_prefix('.')?;
    let suffix = remainder
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '.')
        .collect::<String>();

    if suffix.is_empty() {
        None
    } else {
        Some(format!("params.{param_name}.{suffix}"))
    }
}

fn qualify_manifest_field_path(summary: &str, field: &str) -> String {
    if matches!(field, "duration" | "fps" | "resolution" | "color_space") {
        return format!("environment.{field}");
    }

    if summary.contains("expected one of `resolution`, `fps`, `duration`, `color_space`") {
        return format!("environment.{field}");
    }

    field.to_owned()
}

fn extract_quoted_token_after(summary: &str, marker: &str) -> Option<String> {
    for quote in ['`', '\''] {
        let needle = format!("{marker} {quote}");
        if let Some(index) = summary.find(&needle) {
            let start = index + needle.len();
            let rest = &summary[start..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_owned());
            }
        }
    }
    None
}

fn extract_between<'a>(value: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let (_, tail) = value.split_once(start)?;
    let (captured, _) = tail.split_once(end)?;
    Some(captured)
}

fn extract_yaml_location(error: &Error) -> (Option<u64>, Option<u64>) {
    for cause in error.chain() {
        if let Some(yaml_error) = cause.downcast_ref::<serde_yaml::Error>() {
            if let Some(location) = yaml_error.location() {
                let line = u64::try_from(location.line()).ok();
                let column = u64::try_from(location.column()).ok();
                return (line, column);
            }
        }
    }
    (None, None)
}

fn extract_manifest_path(head: &str) -> Option<String> {
    head.strip_prefix("failed to decode manifest ")
        .map(ToOwned::to_owned)
}
