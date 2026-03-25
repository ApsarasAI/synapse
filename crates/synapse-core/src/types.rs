use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteRequest {
    pub language: String,
    pub code: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_memory_limit_mb")]
    pub memory_limit_mb: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

impl ExecuteResponse {
    pub fn mock_ok() -> Self {
        Self {
            stdout: "synapse initialized".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 1,
        }
    }
}

const fn default_timeout_ms() -> u64 {
    5_000
}

const fn default_memory_limit_mb() -> u32 {
    128
}
