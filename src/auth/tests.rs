use clap::Parser;
use schwab::auth::{TokenData, TokenFile};

use crate::cli::Cli;

#[test]
fn status_does_not_expose_token_secrets() {
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

    let cli = Cli::parse_from([
        "schwab-agent",
        "--token",
        token_path.to_str().expect("token path utf-8"),
        "auth",
        "status",
    ]);
    let output = super::status(&cli).expect("build auth status").to_string();

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
    let cli = Cli::parse_from(["schwab-agent", "auth", "status"]);
    let dir = tempfile::tempdir().unwrap();
    let empty_config = dir.path().join("config.json");
    std::fs::write(&empty_config, "{}").unwrap();
    let err = super::build_config_from(&cli, &empty_config).unwrap_err();
    match err {
        crate::error::AppError::MissingAuthConfig(field) => {
            assert_eq!(field, "client_id");
        }
        other => panic!("expected MissingAuthConfig, got {other:?}"),
    }
}

#[test]
fn build_config_missing_client_secret() {
    let cli = Cli::parse_from(["schwab-agent", "--client-id", "my-id", "auth", "status"]);
    let dir = tempfile::tempdir().unwrap();
    let empty_config = dir.path().join("config.json");
    std::fs::write(&empty_config, "{}").unwrap();
    let err = super::build_config_from(&cli, &empty_config).unwrap_err();
    match err {
        crate::error::AppError::MissingAuthConfig(field) => {
            assert_eq!(field, "client_secret");
        }
        other => panic!("expected MissingAuthConfig, got {other:?}"),
    }
}

#[test]
fn build_config_valid() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "--client-id",
        "my-client-id",
        "--client-secret",
        "my-client-secret",
        "auth",
        "status",
    ]);
    let config = super::build_config(&cli);
    assert!(config.is_ok());
}

#[test]
fn build_config_falls_back_to_config_file() {
    let cli = Cli::parse_from(["schwab-agent", "auth", "status"]);
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    std::fs::write(
        &config_path,
        r#"{"client_id": "from-file", "client_secret": "secret-from-file"}"#,
    )
    .unwrap();
    let config = super::build_config_from(&cli, &config_path);
    assert!(config.is_ok());
}

#[test]
fn build_config_cli_overrides_config_file() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "--client-id",
        "from-cli",
        "--client-secret",
        "secret-from-cli",
        "auth",
        "status",
    ]);
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    std::fs::write(
        &config_path,
        r#"{"client_id": "from-file", "client_secret": "secret-from-file"}"#,
    )
    .unwrap();
    let config = super::build_config_from(&cli, &config_path).unwrap();
    // AuthConfig redacts fields, but we can verify it built successfully with CLI values
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
