//! Agent configuration loaded from the shared config file.
//!
//! The agent reads `~/.config/schwab-agent/config.json` (or
//! `$XDG_CONFIG_HOME/schwab-agent/config.json`) which is shared with the Go
//! CLI. The config file is optional; missing files or missing keys default to
//! safe values (mutable operations disabled).

use std::path::PathBuf;

use serde::Deserialize;

use crate::error::AppError;

/// The default OAuth callback URL used when no CLI arg, env var, or config
/// file provides one.
pub(crate) const DEFAULT_CALLBACK_URL: &str = "https://127.0.0.1:8182";

/// Subset of the shared agent config relevant to this CLI.
///
/// Unknown keys are silently ignored so the Go CLI can add fields without
/// breaking the Rust CLI.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct AgentConfig {
    /// Schwab app client ID, shared with the Go CLI.
    pub client_id: Option<String>,

    /// Schwab app client secret, shared with the Go CLI.
    pub client_secret: Option<String>,

    /// OAuth callback URL registered with Schwab.
    pub callback_url: Option<String>,

    /// When `true`, mutable order operations (place, replace, cancel) are
    /// allowed. Defaults to `false` when the key is absent or the config
    /// file does not exist.
    #[serde(
        default,
        rename = "i-also-like-to-live-dangerously",
        alias = "i_also_like_to_live_dangerously"
    )]
    pub i_also_like_to_live_dangerously: bool,
}

/// Returns the path to the shared agent config file.
///
/// Uses `$XDG_CONFIG_HOME/schwab-agent/config.json`, falling back to
/// `~/.config/schwab-agent/config.json` on platforms without `XDG_CONFIG_HOME`.
#[must_use]
fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("schwab-agent")
        .join("config.json")
}

/// Loads the agent config from a specific path.
///
/// Returns `AgentConfig::default()` (all flags false) when the file is
/// missing, which makes "file not found" a safe no-op rather than an error.
pub(crate) fn load_agent_config_from(path: &std::path::Path) -> Result<AgentConfig, AppError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AgentConfig::default()),
        Err(e) => Err(AppError::Io(e)),
    }
}

/// Loads the agent config from the default shared config path.
pub(crate) fn load_agent_config() -> Result<AgentConfig, AppError> {
    load_agent_config_from(&config_path())
}

/// Checks that mutable operations are enabled, loading config from the
/// given path. Used by tests to avoid depending on the real config file.
#[cfg(test)]
fn require_mutable_enabled_from(path: &std::path::Path) -> Result<(), AppError> {
    let config = load_agent_config_from(path)?;
    if config.i_also_like_to_live_dangerously {
        Ok(())
    } else {
        Err(AppError::MutableDisabled)
    }
}

/// Checks that mutable operations are enabled in the agent config.
///
/// Call this guard at the top of every mutable command handler (place,
/// replace, cancel). Returns `Ok(())` when the flag is set, or
/// `Err(AppError::MutableDisabled)` otherwise.
pub(crate) fn require_mutable_enabled() -> Result<(), AppError> {
    let config = load_agent_config()?;
    if config.i_also_like_to_live_dangerously {
        Ok(())
    } else {
        Err(AppError::MutableDisabled)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn loads_config_with_flag_true() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"i-also-like-to-live-dangerously": true}}"#).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let config: AgentConfig = serde_json::from_str(&contents).unwrap();
        assert!(config.i_also_like_to_live_dangerously);
    }

    #[test]
    fn loads_config_with_flag_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"i-also-like-to-live-dangerously": false}}"#).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let config: AgentConfig = serde_json::from_str(&contents).unwrap();
        assert!(!config.i_also_like_to_live_dangerously);
    }

    #[test]
    fn loads_config_with_flag_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"client_id": "test"}}"#).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let config: AgentConfig = serde_json::from_str(&contents).unwrap();
        assert!(!config.i_also_like_to_live_dangerously);
    }

    #[test]
    fn default_config_has_flag_false() {
        let config = AgentConfig::default();
        assert!(!config.i_also_like_to_live_dangerously);
    }

    #[test]
    fn deserialize_ignores_unknown_keys() {
        let json = r#"{"client_id": "x", "callback_url": "https://localhost", "i-also-like-to-live-dangerously": true}"#;
        let config: AgentConfig = serde_json::from_str(json).unwrap();
        assert!(config.i_also_like_to_live_dangerously);
    }

    #[test]
    fn config_path_ends_with_expected_suffix() {
        let path = config_path();
        assert!(
            path.ends_with("schwab-agent/config.json"),
            "unexpected config path: {path:?}"
        );
    }

    #[test]
    fn require_mutable_returns_error_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"i-also-like-to-live-dangerously": false}}"#).unwrap();

        let result = require_mutable_enabled_from(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), "config.mutable_disabled");
        assert_eq!(err.exit_code(), 10);
        assert!(err.hint().is_some());
    }

    #[test]
    fn require_mutable_returns_error_when_config_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let result = require_mutable_enabled_from(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), "config.mutable_disabled");
    }

    #[test]
    fn require_mutable_returns_ok_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"i-also-like-to-live-dangerously": true}}"#).unwrap();

        let result = require_mutable_enabled_from(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn deserializes_credential_fields() {
        let json = r#"{
            "client_id": "my_id",
            "client_secret": "my_secret",
            "callback_url": "https://localhost:9999"
        }"#;
        let config: AgentConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.client_id.as_deref(), Some("my_id"));
        assert_eq!(config.client_secret.as_deref(), Some("my_secret"));
        assert_eq!(
            config.callback_url.as_deref(),
            Some("https://localhost:9999")
        );
    }

    #[test]
    fn credential_fields_default_to_none() {
        let config = AgentConfig::default();
        assert!(config.client_id.is_none());
        assert!(config.client_secret.is_none());
        assert!(config.callback_url.is_none());
    }
}
