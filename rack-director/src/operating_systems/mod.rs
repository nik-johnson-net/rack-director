pub mod store;

pub use store::OperatingSystemsStore;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An Operating System that can be installed on devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatingSystem {
    pub id: Option<i64>,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Architecture-specific configuration for an Operating System
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsArchitecture {
    pub id: Option<i64>,
    pub os_id: i64,
    pub architecture: Architecture,
    pub kernel_path: String,
    pub initramfs_path: String,
    pub modules: Vec<String>,
    pub cmdline_args: Option<String>,
    pub install_script_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Supported CPU architectures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    #[serde(rename = "x86-64")]
    X86_64,
}

impl Architecture {
    pub fn as_str(&self) -> &'static str {
        match self {
            Architecture::X86_64 => "x86-64",
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "x86-64" => Ok(Architecture::X86_64),
            _ => Err(anyhow!("Unknown architecture: {}", s)),
        }
    }
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
