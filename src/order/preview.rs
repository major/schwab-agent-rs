//! Order preview digest system.
//!
//! Provides a tamper-evident preview/confirm/execute workflow for orders.
//! When previewing, the order payload and account are hashed into a SHA-256
//! digest and stored on disk. When placing from a saved preview, the stored
//! payload is loaded, re-hashed for integrity verification, and submitted
//! exactly as previewed.
//!
//! Works with both option orders and equity orders: the `order` field stores
//! a [`serde_json::Value`] so any serializable order type can round-trip
//! through the preview system.
//!
//! Digests expire after 15 minutes.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::AppError;

/// Time-to-live for saved previews (seconds).
const PREVIEW_TTL_SECS: i64 = 900; // 15 minutes

/// Expected length of a hex-encoded SHA-256 digest.
const DIGEST_HEX_LEN: usize = 64;

/// Saved preview payload stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPreview {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Account hash the order targets.
    pub account_hash: String,
    /// The order payload exactly as previewed, stored as a JSON value.
    pub order: Value,
    /// Command name (e.g., `"order.preview.long-call"`, `"stock.preview.buy"`).
    pub command: String,
    /// Unix timestamp (seconds) when the preview was saved.
    pub saved_at: i64,
}

/// Returns the directory where preview digests are stored.
///
/// Creates the directory if it does not exist.
fn preview_dir() -> Result<PathBuf, AppError> {
    let base = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .ok_or(AppError::Preview(
            "cannot determine state directory".to_string(),
        ))?;
    let dir = base.join("schwab-agent").join("previews");
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Preview(format!("failed to create preview directory: {e}")))?;
    Ok(dir)
}

/// Computes the SHA-256 digest of a preview payload.
///
/// The digest covers the canonical JSON serialization of the payload,
/// binding account, order structure, and metadata together.
fn compute_digest(payload: &SavedPreview) -> Result<String, AppError> {
    let json = serde_json::to_string(payload)
        .map_err(|e| AppError::Preview(format!("failed to serialize preview payload: {e}")))?;
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

/// Saves an order preview to disk and returns its digest.
///
/// The digest is the hex-encoded SHA-256 of the canonical JSON payload.
/// The payload file is named `{digest}.json` in the preview directory.
///
/// Accepts any `Serialize` order type (option orders, equity orders, or raw
/// JSON values). The order is serialized to a [`serde_json::Value`] before
/// storage so that loading does not depend on the original Rust type.
pub fn save_preview<T: Serialize>(
    account_hash: &str,
    order: &T,
    command: &str,
) -> Result<String, AppError> {
    let order_value = serde_json::to_value(order)
        .map_err(|e| AppError::Preview(format!("failed to serialize order for preview: {e}")))?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let payload = SavedPreview {
        version: 1,
        account_hash: account_hash.to_string(),
        order: order_value,
        command: command.to_string(),
        saved_at: now,
    };

    let digest = compute_digest(&payload)?;
    let path = preview_dir()?.join(format!("{digest}.json"));

    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| AppError::Preview(format!("failed to serialize preview: {e}")))?;
    fs::write(&path, &json)
        .map_err(|e| AppError::Preview(format!("failed to write preview file: {e}")))?;

    Ok(digest)
}

/// Loads a previously saved order preview by its digest.
///
/// Validates:
/// - Digest format (64-character hex string)
/// - File exists
/// - Re-derived digest matches (tamper detection)
/// - TTL has not expired (15 minutes)
/// - Account hash matches the provided account
pub fn load_preview(digest: &str, account_hash: &str) -> Result<SavedPreview, AppError> {
    // Validate digest format.
    if digest.len() != DIGEST_HEX_LEN || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::Preview(format!(
            "invalid digest format: expected {DIGEST_HEX_LEN}-character hex string, \
             got {}-character '{digest}'",
            digest.len()
        )));
    }

    // Load file.
    let path = preview_dir()?.join(format!("{digest}.json"));
    let json = fs::read_to_string(&path)
        .map_err(|e| AppError::Preview(format!("preview {digest} not found or unreadable: {e}")))?;

    let payload: SavedPreview = serde_json::from_str(&json)
        .map_err(|e| AppError::Preview(format!("preview {digest} has corrupt data: {e}")))?;

    // Verify integrity (re-derive digest).
    let verified = compute_digest(&payload)?;
    if verified != digest {
        return Err(AppError::Preview(format!(
            "preview integrity check failed: expected {digest}, derived {verified}"
        )));
    }

    // Verify account match.
    if payload.account_hash != account_hash {
        return Err(AppError::Preview(format!(
            "preview was for account '{}', but got account '{account_hash}'",
            payload.account_hash
        )));
    }

    // Check TTL.
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let age = now - payload.saved_at;
    if age > PREVIEW_TTL_SECS {
        // Clean up expired file.
        let _ = fs::remove_file(&path);
        return Err(AppError::Preview(format!(
            "preview {digest} expired ({age}s old, TTL is {PREVIEW_TTL_SECS}s)"
        )));
    }

    Ok(payload)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        path::Path,
        sync::{LazyLock, Mutex},
    };

    use schwab::{Duration, Instruction, PutCall, Session};

    use super::*;
    use crate::order::builder;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);

            unsafe {
                std::env::set_var(key, value);
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

    /// Builds a simple test order value for preview tests.
    fn test_order_value() -> Value {
        let order = builder::build_single_leg(
            "AAPL",
            "2025-01-17",
            200.0,
            1,
            Some(5.0),
            Session::Normal,
            Duration::Day,
            PutCall::Call,
            Instruction::BuyToOpen,
        )
        .unwrap();
        serde_json::to_value(order).unwrap()
    }

    #[test]
    fn round_trip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        // Override preview_dir by using env var or direct file manipulation.
        // Since preview_dir() uses dirs::state_dir(), we test compute_digest
        // and the serialization format instead.
        let order = test_order_value();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let payload = SavedPreview {
            version: 1,
            account_hash: "test-account-hash".to_string(),
            order,
            command: "order.preview.long-call".to_string(),
            saved_at: now,
        };

        // Compute digest.
        let digest = compute_digest(&payload).unwrap();
        assert_eq!(digest.len(), DIGEST_HEX_LEN);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));

        // Save to temp dir.
        let path = dir.path().join(format!("{digest}.json"));
        let json = serde_json::to_string_pretty(&payload).unwrap();
        fs::write(&path, &json).unwrap();

        // Load and verify.
        let loaded: SavedPreview =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let re_digest = compute_digest(&loaded).unwrap();
        assert_eq!(digest, re_digest);
        assert_eq!(loaded.account_hash, "test-account-hash");
    }

    #[test]
    fn digest_changes_with_account() {
        let order = test_order_value();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();

        let p1 = SavedPreview {
            version: 1,
            account_hash: "account-a".to_string(),
            order: order.clone(),
            command: "order.preview.long-call".to_string(),
            saved_at: now,
        };
        let p2 = SavedPreview {
            version: 1,
            account_hash: "account-b".to_string(),
            order,
            command: "order.preview.long-call".to_string(),
            saved_at: now,
        };

        let d1 = compute_digest(&p1).unwrap();
        let d2 = compute_digest(&p2).unwrap();
        assert_ne!(d1, d2);
    }

    #[test]
    fn load_rejects_bad_digest_format() {
        let result = load_preview("not-a-valid-hex", "account");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid digest format"));
    }

    #[test]
    fn load_rejects_account_mismatch() {
        let _guard = ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _state_home = EnvVarGuard::set_path("XDG_STATE_HOME", temp_dir.path());

        let result = (|| {
            let digest = save_preview("HASH_A", &test_order_value(), "order.preview.long-call")?;
            load_preview(&digest, "HASH_B")
        })();

        let err = result.unwrap_err();
        assert_eq!(err.exit_code(), 11);
        match err {
            AppError::Preview(message) => {
                assert!(message.contains("preview was for account 'HASH_A'"));
                assert!(message.contains("got account 'HASH_B'"));
            }
            other => panic!("expected AppError::Preview, got {other:?}"),
        }
    }
}
