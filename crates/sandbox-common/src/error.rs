use thiserror::Error;

pub type Result<T> = std::result::Result<T, SandboxError>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServiceError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SandboxError {
    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("policy validation failed: {0}")]
    PolicyValidation(String),

    #[error("operation denied by policy: {0}")]
    Denied(String),

    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("system operation failed: {0}")]
    System(String),

    #[error("serialization failed: {0}")]
    Serialization(String),
}

impl SandboxError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "CONFIGURATION",
            Self::PolicyValidation(_) => "POLICY_VALIDATION",
            Self::Denied(_) => "DENIED",
            Self::UnsupportedPlatform(_) => "UNSUPPORTED_PLATFORM",
            Self::System(_) => "SYSTEM",
            Self::Serialization(_) => "SERIALIZATION",
        }
    }

    pub fn to_service_error(&self) -> ServiceError {
        ServiceError {
            code: self.code().to_string(),
            message: self.to_string(),
        }
    }
}

impl From<SandboxError> for ServiceError {
    fn from(value: SandboxError) -> Self {
        value.to_service_error()
    }
}

impl From<serde_json::Error> for SandboxError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}
