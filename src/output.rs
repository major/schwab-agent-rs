use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::AppError;

/// Current output schema version.
pub const SCHEMA_VERSION: u16 = 1;

/// Standard command output type.
pub type CommandOutput = Envelope<Value>;

/// Stable JSON envelope for successful and failed commands.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
pub struct Envelope<T>
where
    T: Serialize,
{
    /// Output schema version.
    pub version: u16,
    /// True when the command completed successfully.
    pub ok: bool,
    /// Dotted command name, such as `market.quote`.
    pub command: Option<String>,
    /// Successful command data.
    pub data: Option<T>,
    /// Error details for failed commands.
    pub error: Option<ErrorBody>,
    /// Non-fatal issues the caller may need to know about.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Metadata common to all command outputs.
    pub meta: Metadata,
}

impl Envelope<Value> {
    /// Builds a success envelope.
    #[must_use]
    pub fn success(command: &str, data: Value, meta: Metadata) -> Self {
        Self {
            version: SCHEMA_VERSION,
            ok: true,
            command: Some(command.to_string()),
            data: Some(data),
            error: None,
            warnings: Vec::new(),
            meta,
        }
    }

    /// Builds an error envelope.
    #[must_use]
    pub fn error(error: ErrorBody) -> Self {
        Self {
            version: SCHEMA_VERSION,
            ok: false,
            command: None,
            data: None,
            error: Some(error),
            warnings: Vec::new(),
            meta: Metadata::now(),
        }
    }
}

/// Metadata common to all command outputs.
#[derive(Debug, Serialize)]
pub struct Metadata {
    /// UTC timestamp when the output was generated.
    pub generated_at: String,
}

impl Metadata {
    /// Builds metadata with the current UTC timestamp.
    #[must_use]
    pub fn now() -> Self {
        let generated_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        Self { generated_at }
    }
}

/// Stable error payload for machine-readable failures.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// Stable error code.
    pub code: &'static str,
    /// Short human-readable error message.
    pub message: String,
    /// Error category for coarse agent decisions.
    pub category: &'static str,
    /// Whether retrying without user action may succeed.
    pub retryable: bool,
    /// Optional remediation hint.
    pub hint: Option<&'static str>,
}

impl From<&AppError> for ErrorBody {
    fn from(error: &AppError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
            category: error.category(),
            retryable: error.retryable(),
            hint: error.hint(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::error::AppError;
    use crate::output::{Envelope, ErrorBody, Metadata, SCHEMA_VERSION};

    #[test]
    fn metadata_now_produces_rfc3339_timestamp() {
        let meta = Metadata::now();
        // RFC3339 timestamps always contain 'T' separating date and time.
        assert!(meta.generated_at.contains('T'));
    }

    #[test]
    fn envelope_success_fields() {
        let meta = Metadata::now();
        let data = json!({"price": 150.0});
        let envelope = Envelope::success("market.quote", data.clone(), meta);

        assert_eq!(envelope.version, SCHEMA_VERSION);
        assert!(envelope.ok);
        assert_eq!(envelope.command.as_deref(), Some("market.quote"));
        assert_eq!(envelope.data, Some(data));
        assert!(envelope.error.is_none());
        assert!(envelope.warnings.is_empty());
    }

    #[test]
    fn envelope_error_fields() {
        let app_err = AppError::MissingAuthConfig("client_id");
        let body = ErrorBody::from(&app_err);
        let envelope = Envelope::error(body);

        assert_eq!(envelope.version, SCHEMA_VERSION);
        assert!(!envelope.ok);
        assert!(envelope.command.is_none());
        assert!(envelope.data.is_none());
        assert!(envelope.error.is_some());
        assert!(envelope.warnings.is_empty());
    }

    #[test]
    fn error_body_from_app_error_maps_all_fields() {
        let app_err = AppError::TokenFileMissing("/tmp/token.json".to_string());
        let body = ErrorBody::from(&app_err);

        assert_eq!(body.code, "auth.token_missing");
        assert!(body.message.contains("token file not found"));
        assert_eq!(body.category, "auth");
        assert!(!body.retryable);
        assert!(body.hint.is_some());
    }

    #[test]
    fn success_envelope_serializes_without_error_field() {
        let meta = Metadata::now();
        let envelope = Envelope::success("auth.status", json!({}), meta);
        let serialized = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

        assert!(parsed.get("error").is_none());
        assert!(parsed.get("data").is_some());
        assert_eq!(parsed["ok"], true);
    }

    #[test]
    fn error_envelope_serializes_without_data_field() {
        let app_err = AppError::Io(std::io::Error::other("test"));
        let body = ErrorBody::from(&app_err);
        let envelope = Envelope::error(body);
        let serialized = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

        assert!(parsed.get("data").is_none());
        assert!(parsed.get("error").is_some());
        assert_eq!(parsed["ok"], false);
    }

    #[test]
    fn error_body_without_hint_omits_hint_in_json() {
        let app_err = AppError::Io(std::io::Error::other("oops"));
        let body = ErrorBody::from(&app_err);
        let serialized = serde_json::to_string(&body).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

        assert!(parsed.get("hint").is_none());
    }
}
