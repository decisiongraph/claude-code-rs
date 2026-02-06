use serde::{Deserialize, Serialize};

/// Sandbox configuration for the CLI process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// Type of sandbox to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_type: Option<SandboxType>,

    /// Network access allowed.
    #[serde(default)]
    pub allow_network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SandboxType {
    None,
    Docker,
    Firecracker,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            sandbox_type: None,
            allow_network: true,
        }
    }
}
