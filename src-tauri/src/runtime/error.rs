use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

use mouser_platform::PlatformError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RuntimeError {
    #[error("runtime state is unavailable")]
    StateUnavailable,
    #[error("{message}")]
    Internal { message: String },
    #[error("{operation} failed: {message}")]
    OperationFailed { operation: String, message: String },
    #[error("{operation} failed: {message}")]
    Platform { operation: String, message: String },
    #[error("legacy import failed: {message}")]
    LegacyImport { message: String },
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;

impl RuntimeError {
    pub fn operation(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::OperationFailed {
            operation: operation.into(),
            message: message.into(),
        }
    }

    pub fn platform(operation: impl Into<String>, error: PlatformError) -> Self {
        Self::Platform {
            operation: operation.into(),
            message: error.to_string(),
        }
    }
}

impl From<String> for RuntimeError {
    fn from(message: String) -> Self {
        Self::Internal { message }
    }
}
