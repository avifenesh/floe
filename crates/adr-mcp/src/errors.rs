use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Frozen error-code set (scope 3, week 6 freeze). The code set is
/// ABI — the GLM / Qwen prompts parse these strings directly, so any
/// rename breaks deployed artifacts. Treat like a wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    NameReserved,
    NameTooShort,
    NameTooLong,
    RationaleTooShort,
    RationaleTooLong,
    HunkNotFound,
    EntityNotFound,
    FlowNotFound,
    CoverageBroken,
    CallBudgetExceeded,
}

/// Wire-level error returned by mutation handlers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{code:?}: {reason}")]
pub struct ToolError {
    pub code: ErrorCode,
    pub reason: String,
}

impl ToolError {
    pub fn new(code: ErrorCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

/// Rejection reason returned by `finalize` when a global invariant fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectReason {
    pub rejected_rule: &'static str,
    pub detail: String,
}
