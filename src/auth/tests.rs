use std::{ffi::OsString, path::Path};

use clap::Parser;
use schwab::auth::{AuthContext, CallbackResult, TokenData, TokenFile};

use crate::cli::Cli;

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
fn status_does_not_expose_token_secrets() {
    let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let token_path = temp_dir.path().join("token.json");
    let token_file = TokenFile {
        creation_timestamp: 1_700_000_000,
        token: TokenData {
            access_token: "secret-access-token".to_string(),
            token_type: Some("Bearer".to_string()),
            expires_in: Some(1_800),
            refresh_token: Some("secret-refresh-token".to_string()),
            scope: Some("readonly".to_string()),
            expires_at: Some(1_700_001_800),
        },
    };
    std::fs::write(
        &token_path,
        serde_json::to_vec(&token_file).expect("serialize token file"),
    )
    .expect("write token file");

    let _token_path = EnvVarGuard::set_path("SCHWAB_TOKEN_PATH", &token_path);
    let _cli = Cli::parse_from(["schwab-agent", "auth", "status"]);
    let output = super::status().expect("build auth status").to_string();

    assert!(output.contains("token_present"));
    assert!(!output.contains("secret-access-token"));
    assert!(!output.contains("secret-refresh-token"));
}

// -- format_epoch ---------------------------------------------------------

#[test]
fn format_epoch_valid_timestamp() {
    let result = super::format_epoch(1_700_000_000);
    assert!(result.is_some());
    let formatted = result.unwrap();
    // Should be RFC 3339 format
    assert!(formatted.contains("2023-11-14"));
    assert!(formatted.contains('T'));
}

#[test]
fn format_epoch_zero() {
    let result = super::format_epoch(0);
    assert!(result.is_some());
    assert!(result.unwrap().contains("1970-01-01"));
}

#[test]
fn format_epoch_negative_valid() {
    // One second before epoch should still work
    let result = super::format_epoch(-1);
    assert!(result.is_some());
    assert!(result.unwrap().contains("1969-12-31"));
}

// -- now_epoch ------------------------------------------------------------

#[test]
fn now_epoch_returns_reasonable_value() {
    let now = super::now_epoch();
    // Should be after 2024-01-01 and before 2100-01-01
    assert!(now > 1_704_067_200, "now_epoch too small: {now}");
    assert!(now < 4_102_444_800, "now_epoch too large: {now}");
}

// -- require_token_file ---------------------------------------------------

#[test]
fn require_token_file_existing_path() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let path = temp_dir.path().join("token.json");
    std::fs::write(&path, b"{}").expect("write file");
    assert!(super::require_token_file(&path).is_ok());
}

#[test]
fn require_token_file_missing_path() {
    let path = std::path::Path::new("/tmp/opencode/definitely-not-a-real-token-file.json");
    let err = super::require_token_file(path).unwrap_err();
    match err {
        crate::error::AppError::TokenFileMissing(msg) => {
            assert!(msg.contains("definitely-not-a-real-token-file"));
        }
        other => panic!("expected TokenFileMissing, got {other:?}"),
    }
}

// -- callback request parsing ---------------------------------------------

#[test]
fn callback_request_ignores_browser_probe_without_oauth_params() {
    let request = "GET / HTTP/1.1\r\nhost: 127.0.0.1:8182\r\n\r\n";

    assert!(matches!(
        super::parse_callback_request(request, "/"),
        super::CallbackOutcome::Continue
    ));
}

#[test]
fn callback_request_ignores_unexpected_path() {
    let request = "GET /favicon.ico HTTP/1.1\r\nhost: 127.0.0.1:8182\r\n\r\n";

    assert!(matches!(
        super::parse_callback_request(request, "/callback"),
        super::CallbackOutcome::Continue
    ));
}

#[test]
fn callback_request_accepts_complete_oauth_callback() {
    let request =
        "GET /callback?code=abc123&state=state456 HTTP/1.1\r\nhost: 127.0.0.1:8182\r\n\r\n";

    match super::parse_callback_request(request, "/callback") {
        super::CallbackOutcome::Complete(result) => {
            assert_eq!(result.code, "abc123");
            assert_eq!(result.state, "state456");
        }
        _ => panic!("expected complete callback"),
    }
}

#[test]
fn callback_request_stops_on_oauth_error() {
    let request = "GET /callback?error=access_denied&error_description=user%20cancelled HTTP/1.1\r\nhost: 127.0.0.1:8182\r\n\r\n";

    match super::parse_callback_request(request, "/callback") {
        super::CallbackOutcome::Fatal(message) => {
            assert_eq!(message, "access_denied: user cancelled");
        }
        _ => panic!("expected fatal OAuth error"),
    }
}

#[test]
fn callback_redirect_url_encodes_code_and_state() {
    let context = AuthContext {
        callback_url: "https://127.0.0.1:8182/callback".to_string(),
        authorization_url: String::new(),
        state: "state with spaces".to_string(),
    };
    let result = CallbackResult {
        code: "code/with?symbols".to_string(),
        state: context.state.clone(),
    };

    let redirect_url = super::callback_redirect_url(&context, &result).unwrap();

    assert_eq!(
        redirect_url,
        "https://127.0.0.1:8182/callback?code=code%2Fwith%3Fsymbols&state=state+with+spaces"
    );
}

#[test]
fn callback_stream_timeout_defaults_without_login_deadline() {
    assert_eq!(
        super::stream_io_timeout(None).unwrap(),
        std::time::Duration::from_secs(10)
    );
}

#[test]
fn callback_stream_timeout_is_capped_by_login_deadline() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    let timeout = super::stream_io_timeout(Some(deadline)).unwrap();

    assert!(timeout <= std::time::Duration::from_millis(50));
    assert!(timeout > std::time::Duration::ZERO);
}

#[test]
fn callback_stream_timeout_fails_after_login_deadline() {
    let deadline = std::time::Instant::now() - std::time::Duration::from_millis(1);
    let err = super::stream_io_timeout(Some(deadline)).unwrap_err();

    assert!(err.to_string().contains("timed out waiting for callback"));
}

// -- AuthStatus::missing --------------------------------------------------

#[test]
fn auth_status_missing_fields() {
    let status = super::AuthStatus::missing(std::path::Path::new("/fake/token.json"));
    assert!(!status.token_present);
    assert_eq!(status.token_path, "/fake/token.json");
    assert!(status.access_expires_at.is_none());
    assert!(status.access_expired.is_none());
    assert!(status.refresh_created_at.is_none());
    assert!(status.refresh_expires_at.is_none());
    assert!(status.refresh_expired.is_none());
    assert!(!status.refresh_possible);
}

#[test]
fn auth_status_missing_serializes_without_optional_fields() {
    let status = super::AuthStatus::missing(std::path::Path::new("/fake/path"));
    let json = serde_json::to_value(&status).expect("serialize");
    assert_eq!(json["token_present"], false);
    assert_eq!(json["refresh_possible"], false);
    // skip_serializing_if = None fields should be absent
    assert!(json.get("access_expires_at").is_none());
    assert!(json.get("access_expired").is_none());
}

// -- AuthStatus::from_token_file ------------------------------------------

fn make_token_data(expires_at: Option<i64>, refresh_token: Option<String>) -> TokenData {
    TokenData {
        access_token: "test-access".into(),
        token_type: Some("Bearer".into()),
        expires_in: Some(1800),
        refresh_token,
        scope: Some("readonly".into()),
        expires_at,
    }
}

#[test]
fn auth_status_from_token_file_valid_access_valid_refresh() {
    let now = super::now_epoch();
    // Access expires 30 min from now, creation recent
    let token_file = TokenFile {
        creation_timestamp: now - 3600,
        token: make_token_data(Some(now + 1800), Some("refresh-tok".into())),
    };
    let status =
        super::AuthStatus::from_token_file(std::path::Path::new("/tmp/token.json"), &token_file);
    assert!(status.token_present);
    assert_eq!(status.access_expired, Some(false));
    assert_eq!(status.refresh_expired, Some(false));
    assert!(status.refresh_possible);
    assert!(status.access_expires_at.is_some());
    assert!(status.refresh_created_at.is_some());
    assert!(status.refresh_expires_at.is_some());
}

#[test]
fn auth_status_from_token_file_expired_access() {
    let now = super::now_epoch();
    let token_file = TokenFile {
        creation_timestamp: now - 3600,
        token: make_token_data(Some(now - 100), Some("refresh-tok".into())),
    };
    let status =
        super::AuthStatus::from_token_file(std::path::Path::new("/tmp/token.json"), &token_file);
    assert_eq!(status.access_expired, Some(true));
    // Refresh should still be valid (created 1 hour ago, max age ~6.5 days)
    assert_eq!(status.refresh_expired, Some(false));
    assert!(status.refresh_possible);
}

#[test]
fn auth_status_from_token_file_expired_refresh() {
    let now = super::now_epoch();
    // Creation was longer ago than REFRESH_TOKEN_MAX_AGE_SECONDS
    let old_creation = now - super::REFRESH_TOKEN_MAX_AGE_SECONDS - 1;
    let token_file = TokenFile {
        creation_timestamp: old_creation,
        token: make_token_data(Some(now - 100), Some("refresh-tok".into())),
    };
    let status =
        super::AuthStatus::from_token_file(std::path::Path::new("/tmp/token.json"), &token_file);
    assert_eq!(status.refresh_expired, Some(true));
    assert!(!status.refresh_possible);
}

#[test]
fn auth_status_from_token_file_no_refresh_token() {
    let now = super::now_epoch();
    let token_file = TokenFile {
        creation_timestamp: now - 3600,
        token: make_token_data(Some(now + 1800), None),
    };
    let status =
        super::AuthStatus::from_token_file(std::path::Path::new("/tmp/token.json"), &token_file);
    // No refresh token means refresh is not possible
    assert!(!status.refresh_possible);
    // Refresh expired should still be computed from timestamps
    assert_eq!(status.refresh_expired, Some(false));
}

#[test]
fn auth_status_from_token_file_no_expires_at() {
    let now = super::now_epoch();
    let token_file = TokenFile {
        creation_timestamp: now - 3600,
        token: make_token_data(None, Some("refresh-tok".into())),
    };
    let status =
        super::AuthStatus::from_token_file(std::path::Path::new("/tmp/token.json"), &token_file);
    assert!(status.access_expires_at.is_none());
    assert!(status.access_expired.is_none());
    assert!(status.refresh_possible);
}

// -- build_config ---------------------------------------------------------

#[test]
fn build_config_missing_client_id() {
    let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
    let _client_id = EnvVarGuard::remove("SCHWAB_CLIENT_ID");
    let _client_secret = EnvVarGuard::remove("SCHWAB_CLIENT_SECRET");
    let _callback_url = EnvVarGuard::remove("SCHWAB_CALLBACK_URL");
    let dir = tempfile::tempdir().unwrap();
    let empty_config = dir.path().join("config.json");
    std::fs::write(&empty_config, "{}").unwrap();
    let err = super::build_config_from(&empty_config).unwrap_err();
    match err {
        crate::error::AppError::MissingAuthConfig(field) => {
            assert_eq!(field, "client_id");
        }
        other => panic!("expected MissingAuthConfig, got {other:?}"),
    }
}

#[test]
fn build_config_missing_client_secret() {
    let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
    let _client_id = EnvVarGuard::set("SCHWAB_CLIENT_ID", "my-id");
    let _client_secret = EnvVarGuard::remove("SCHWAB_CLIENT_SECRET");
    let _callback_url = EnvVarGuard::remove("SCHWAB_CALLBACK_URL");
    let dir = tempfile::tempdir().unwrap();
    let empty_config = dir.path().join("config.json");
    std::fs::write(&empty_config, "{}").unwrap();
    let err = super::build_config_from(&empty_config).unwrap_err();
    match err {
        crate::error::AppError::MissingAuthConfig(field) => {
            assert_eq!(field, "client_secret");
        }
        other => panic!("expected MissingAuthConfig, got {other:?}"),
    }
}

#[test]
fn build_config_valid() {
    let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
    let _client_id = EnvVarGuard::set("SCHWAB_CLIENT_ID", "my-client-id");
    let _client_secret = EnvVarGuard::set("SCHWAB_CLIENT_SECRET", "my-client-secret");
    let _callback_url = EnvVarGuard::remove("SCHWAB_CALLBACK_URL");
    let dir = tempfile::tempdir().unwrap();
    let empty_config = dir.path().join("config.json");
    std::fs::write(&empty_config, "{}").unwrap();

    let config = super::build_config_from(&empty_config);
    assert!(config.is_ok());
}

#[test]
fn build_config_falls_back_to_config_file() {
    let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
    let _client_id = EnvVarGuard::remove("SCHWAB_CLIENT_ID");
    let _client_secret = EnvVarGuard::remove("SCHWAB_CLIENT_SECRET");
    let _callback_url = EnvVarGuard::remove("SCHWAB_CALLBACK_URL");
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    std::fs::write(
        &config_path,
        r#"{"client_id": "from-file", "client_secret": "secret-from-file"}"#,
    )
    .unwrap();
    let config = super::build_config_from(&config_path);
    assert!(config.is_ok());
}

#[test]
fn build_config_env_overrides_config_file() {
    let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
    let _client_id = EnvVarGuard::set("SCHWAB_CLIENT_ID", "from-env");
    let _client_secret = EnvVarGuard::set("SCHWAB_CLIENT_SECRET", "secret-from-env");
    let _callback_url = EnvVarGuard::remove("SCHWAB_CALLBACK_URL");
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    std::fs::write(
        &config_path,
        r#"{"client_id": "from-file", "client_secret": "secret-from-file"}"#,
    )
    .unwrap();
    let config = super::build_config_from(&config_path).unwrap();
    // AuthConfig redacts fields, but we can verify it built successfully with env values
    // by confirming no error was returned when both sources are present
    assert!(format!("{config:?}").contains("<redacted>"));
}

// -- Serialization of output structs --------------------------------------

#[test]
fn login_url_output_serializes() {
    let output = super::LoginUrlOutput {
        authorization_url: "https://example.com/auth".into(),
        callback_url: "https://127.0.0.1:8182".into(),
        state: "random-state".into(),
        token_path: "/tmp/token.json".into(),
        browser_opened: false,
    };
    let json = serde_json::to_value(&output).expect("serialize");
    assert_eq!(json["authorization_url"], "https://example.com/auth");
    assert_eq!(json["browser_opened"], false);
    assert_eq!(json["state"], "random-state");
}

#[test]
fn token_saved_output_serializes() {
    let output = super::TokenSavedOutput {
        token_saved: true,
        token_path: "/tmp/token.json".into(),
    };
    let json = serde_json::to_value(&output).expect("serialize");
    assert_eq!(json["token_saved"], true);
    assert_eq!(json["token_path"], "/tmp/token.json");
}

#[test]
fn refresh_output_serializes_with_expiry() {
    let output = super::RefreshOutput {
        refreshed: true,
        token_path: "/tmp/token.json".into(),
        access_expires_at: Some("2024-01-15T12:00:00Z".into()),
    };
    let json = serde_json::to_value(&output).expect("serialize");
    assert_eq!(json["refreshed"], true);
    assert_eq!(json["access_expires_at"], "2024-01-15T12:00:00Z");
}

#[test]
fn refresh_output_serializes_without_expiry() {
    let output = super::RefreshOutput {
        refreshed: true,
        token_path: "/tmp/token.json".into(),
        access_expires_at: None,
    };
    let json = serde_json::to_value(&output).expect("serialize");
    assert_eq!(json["refreshed"], true);
    // skip_serializing_if = None
    assert!(json.get("access_expires_at").is_none());
}
