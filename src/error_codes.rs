use std::fmt;

use anyhow::Error;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodedErrorKind {
    Usage,
}

#[derive(Debug, Clone)]
pub struct CodedError {
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
    pub kind: CodedErrorKind,
}

impl CodedError {
    pub fn usage(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            kind: CodedErrorKind::Usage,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            ok: false,
            error: ErrorEnvelopeBody {
                code: self.code.to_owned(),
                message: self.message.clone(),
                details: self.details.clone(),
            },
        }
    }
}

impl fmt::Display for CodedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CodedError {}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelope {
    pub ok: bool,
    pub error: ErrorEnvelopeBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelopeBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

pub fn find_coded_error(error: &Error) -> Option<&CodedError> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<CodedError>())
}
