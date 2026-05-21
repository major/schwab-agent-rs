//! Agent configuration loaded from the shared config file.
//!
//! The agent reads `~/.config/schwab-agent/config.json` (or
//! `$XDG_CONFIG_HOME/schwab-agent/config.json`) which is shared with the Go
//! CLI. The config file is optional; missing files or missing keys default to
//! safe values (mutable operations disabled). Credentials and token paths can
//! also be provided with environment variables.

use std::path::{Path, PathBuf};

#[cfg(test)]
use std::sync::{LazyLock, Mutex};

use serde::Deserialize;

use crate::error::AppError;

/// Shared lock for tests that mutate process-wide environment variables.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// The default OAuth callback URL used when no env var or config file provides one.
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
    xdg_config_home()
        .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config")))
        .join("schwab-agent")
        .join("config.json")
}

/// Returns `$XDG_CONFIG_HOME` as a [`PathBuf`] when the env var is set to a
/// non-empty value, regardless of platform. This lets tests inject a temp dir
/// on macOS where [`dirs::config_dir`] would otherwise resolve to
/// `~/Library/Application Support` and ignore `XDG_CONFIG_HOME`.
fn xdg_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Returns the default OAuth token path.
#[must_use]
fn default_token_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("schwab-agent-rs")
        .join("token.json")
}

/// Returns the OAuth token path from `SCHWAB_TOKEN_PATH`, falling back to the
/// default path under the user's config directory.
#[must_use]
pub(crate) fn token_path() -> PathBuf {
    std::env::var_os("SCHWAB_TOKEN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_token_path)
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

/// Resolves Schwab API credentials from environment variables and the shared
/// agent config file.
///
/// Environment variables take precedence over the config file. The callback URL
/// falls back to [`DEFAULT_CALLBACK_URL`] when neither source provides one.
pub(crate) fn resolve_credentials() -> Result<(String, String, String), AppError> {
    resolve_credentials_from(&config_path())
}

/// Testable variant of [`resolve_credentials`] that loads the agent config from
/// a specific path instead of the default location.
pub(crate) fn resolve_credentials_from(path: &Path) -> Result<(String, String, String), AppError> {
    let config = load_agent_config_from(path)?;
    let client_id = std::env::var("SCHWAB_CLIENT_ID")
        .ok()
        .or(config.client_id)
        .ok_or(AppError::MissingAuthConfig("client_id"))?;
    let client_secret = std::env::var("SCHWAB_CLIENT_SECRET")
        .ok()
        .or(config.client_secret)
        .ok_or(AppError::MissingAuthConfig("client_secret"))?;
    let callback_url = std::env::var("SCHWAB_CALLBACK_URL")
        .ok()
        .or(config.callback_url)
        .unwrap_or_else(|| DEFAULT_CALLBACK_URL.to_string());

    Ok((client_id, client_secret, callback_url))
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
    use std::{ffi::OsString, io::Write, path::Path};

    use super::*;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

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
    fn token_path_from_env() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let _guard = EnvVarGuard::set_path("SCHWAB_TOKEN_PATH", &path);

        assert_eq!(token_path(), path);
    }

    #[test]
    fn token_path_default_fallback() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _guard = EnvVarGuard::remove("SCHWAB_TOKEN_PATH");

        let path = token_path();

        assert!(path.ends_with("schwab-agent-rs/token.json"));
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
    fn load_agent_config_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");

        let config = load_agent_config_from(&path).expect("missing config should be safe default");

        assert!(config.client_id.is_none());
        assert!(config.client_secret.is_none());
        assert!(config.callback_url.is_none());
        assert!(!config.i_also_like_to_live_dangerously);
    }

    #[test]
    fn load_agent_config_rejects_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{not json").unwrap();

        let err = load_agent_config_from(&path).unwrap_err();

        assert_eq!(err.code(), "json.error");
        assert_eq!(err.exit_code(), 20);
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

    #[test]
    fn resolve_credentials_from_env() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _client_id = EnvVarGuard::set("SCHWAB_CLIENT_ID", "env-id");
        let _client_secret = EnvVarGuard::set("SCHWAB_CLIENT_SECRET", "env-secret");
        let _callback_url = EnvVarGuard::set("SCHWAB_CALLBACK_URL", "https://env.example");
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let (client_id, client_secret, callback_url) =
            resolve_credentials_from(&config_path).unwrap();

        assert_eq!(client_id, "env-id");
        assert_eq!(client_secret, "env-secret");
        assert_eq!(callback_url, "https://env.example");
    }

    #[test]
    fn resolve_credentials_env_overrides_config() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _client_id = EnvVarGuard::set("SCHWAB_CLIENT_ID", "env-id");
        let _client_secret = EnvVarGuard::set("SCHWAB_CLIENT_SECRET", "env-secret");
        let _callback_url = EnvVarGuard::set("SCHWAB_CALLBACK_URL", "https://env.example");
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(
            &config_path,
            r#"{"client_id":"file-id","client_secret":"file-secret","callback_url":"https://file.example"}"#,
        )
        .unwrap();

        let (client_id, client_secret, callback_url) =
            resolve_credentials_from(&config_path).unwrap();

        assert_eq!(client_id, "env-id");
        assert_eq!(client_secret, "env-secret");
        assert_eq!(callback_url, "https://env.example");
    }

    #[test]
    fn resolve_credentials_from_config_file() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _client_id = EnvVarGuard::remove("SCHWAB_CLIENT_ID");
        let _client_secret = EnvVarGuard::remove("SCHWAB_CLIENT_SECRET");
        let _callback_url = EnvVarGuard::remove("SCHWAB_CALLBACK_URL");
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(
            &config_path,
            r#"{"client_id":"file-id","client_secret":"file-secret","callback_url":"https://file.example"}"#,
        )
        .unwrap();

        let (client_id, client_secret, callback_url) =
            resolve_credentials_from(&config_path).unwrap();

        assert_eq!(client_id, "file-id");
        assert_eq!(client_secret, "file-secret");
        assert_eq!(callback_url, "https://file.example");
    }

    #[test]
    fn resolve_credentials_missing_client_id() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _client_id = EnvVarGuard::remove("SCHWAB_CLIENT_ID");
        let _client_secret = EnvVarGuard::remove("SCHWAB_CLIENT_SECRET");
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(&config_path, "{}").unwrap();

        let err = resolve_credentials_from(&config_path).unwrap_err();

        match err {
            AppError::MissingAuthConfig(field) => assert_eq!(field, "client_id"),
            other => panic!("expected MissingAuthConfig, got {other:?}"),
        }
    }

    #[test]
    fn resolve_credentials_missing_client_secret() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _client_id = EnvVarGuard::set("SCHWAB_CLIENT_ID", "env-id");
        let _client_secret = EnvVarGuard::remove("SCHWAB_CLIENT_SECRET");
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(&config_path, "{}").unwrap();

        let err = resolve_credentials_from(&config_path).unwrap_err();

        match err {
            AppError::MissingAuthConfig(field) => assert_eq!(field, "client_secret"),
            other => panic!("expected MissingAuthConfig, got {other:?}"),
        }
    }

    #[test]
    fn resolve_credentials_callback_url_default() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let _client_id = EnvVarGuard::remove("SCHWAB_CLIENT_ID");
        let _client_secret = EnvVarGuard::remove("SCHWAB_CLIENT_SECRET");
        let _callback_url = EnvVarGuard::remove("SCHWAB_CALLBACK_URL");
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(
            &config_path,
            r#"{"client_id":"file-id","client_secret":"file-secret"}"#,
        )
        .unwrap();

        let (_, _, callback_url) = resolve_credentials_from(&config_path).unwrap();

        assert_eq!(callback_url, DEFAULT_CALLBACK_URL);
    }
}
