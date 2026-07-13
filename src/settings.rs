//! Daemon-owned user settings for scratch storage and context scope.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Where new scratch artifacts should be written.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScratchStorageMode {
    /// Durable storage under the harnessd runtime directory.
    Runtime,
    /// Ephemeral storage under the operating system temp directory.
    Temp,
}

impl ScratchStorageMode {
    /// Return the stable wire-format label for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Temp => "temp",
        }
    }
}

impl Default for ScratchStorageMode {
    fn default() -> Self {
        Self::Runtime
    }
}

impl std::str::FromStr for ScratchStorageMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "runtime" => Ok(Self::Runtime),
            "temp" => Ok(Self::Temp),
            other => anyhow::bail!("unsupported scratch storage mode: {other}"),
        }
    }
}

/// How much project context Codex may receive by default.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadScope {
    /// Only the current file/selection plus explicitly requested context.
    CurrentContext,
}

impl ReadScope {
    /// Return the stable wire-format label for this scope.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CurrentContext => "current_context",
        }
    }
}

impl Default for ReadScope {
    fn default() -> Self {
        Self::CurrentContext
    }
}

impl std::str::FromStr for ReadScope {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "current_context" => Ok(Self::CurrentContext),
            other => anyhow::bail!("unsupported read scope: {other}"),
        }
    }
}

/// Persisted daemon settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct HarnessSettings {
    /// Scratch storage location mode.
    #[serde(default)]
    pub scratch_storage_mode: ScratchStorageMode,
    /// Default context sharing scope.
    #[serde(default)]
    pub read_scope: ReadScope,
}

impl Default for HarnessSettings {
    fn default() -> Self {
        Self {
            scratch_storage_mode: ScratchStorageMode::Runtime,
            read_scope: ReadScope::CurrentContext,
        }
    }
}

/// Parameters for updating daemon settings.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct SettingsUpdateParams {
    /// Optional replacement scratch storage mode.
    #[serde(default)]
    pub scratch_storage_mode: Option<ScratchStorageMode>,
    /// Optional replacement read scope.
    #[serde(default)]
    pub read_scope: Option<ReadScope>,
}

/// Result returned by settings RPC methods.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SettingsResult {
    /// Current settings after reading or updating.
    pub settings: HarnessSettings,
}

/// JSON-backed settings store.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    /// Create a settings store at the supplied runtime path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load current settings or return defaults when none have been saved.
    pub fn load(&self) -> anyhow::Result<HarnessSettings> {
        if !self.path.exists() {
            return Ok(HarnessSettings::default());
        }
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.path.display()))
    }

    /// Return current settings in RPC result shape.
    pub fn get(&self) -> anyhow::Result<SettingsResult> {
        Ok(SettingsResult {
            settings: self.load()?,
        })
    }

    /// Apply partial settings updates and persist the result.
    pub fn update(&self, params: &SettingsUpdateParams) -> anyhow::Result<SettingsResult> {
        let mut settings = self.load()?;
        if let Some(mode) = params.scratch_storage_mode {
            settings.scratch_storage_mode = mode;
        }
        if let Some(scope) = params.read_scope {
            settings.read_scope = scope;
        }
        self.save(&settings)?;
        Ok(SettingsResult { settings })
    }

    fn save(&self, settings: &HarnessSettings) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(settings)?)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }
}
