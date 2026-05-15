use crate::error::AppError;

fn json_error() -> serde_json::Error {
    serde_json::from_str::<serde_json::Value>("{").unwrap_err()
}

// -- AppError variant classification -------------------------------------------

#[test]
fn missing_auth_config_classification() {
    let err = AppError::MissingAuthConfig("client_id");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.code(), "auth.config_missing");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert!(err.hint().unwrap().contains("SCHWAB_CLIENT_ID"));
}

#[test]
fn token_file_missing_classification() {
    let err = AppError::TokenFileMissing("/tmp/token.json".to_string());
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.code(), "auth.token_missing");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert!(err.hint().unwrap().contains("auth login-url"));
}

#[test]
fn io_error_classification() {
    let err = AppError::Io(std::io::Error::other("disk full"));
    assert_eq!(err.exit_code(), 20);
    assert_eq!(err.code(), "io.error");
    assert_eq!(err.category(), "io");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn json_error_classification() {
    let err = AppError::Json(json_error());
    assert_eq!(err.exit_code(), 20);
    assert_eq!(err.code(), "json.error");
    assert_eq!(err.category(), "json");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn options_symbol_not_found_classification() {
    let err = AppError::OptionsSymbolNotFound {
        symbol: "INVALID".to_string(),
    };
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "options.symbol_not_found");
    assert_eq!(err.category(), "options");
    assert!(!err.retryable());
    assert_eq!(
        err.hint(),
        Some("Check that the symbol is correct and has listed options")
    );
}

#[test]
fn options_validation_classification() {
    let err = AppError::OptionsValidation {
        message: "invalid field: foo".to_string(),
    };
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "options.validation_failed");
    assert_eq!(err.category(), "options");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn account_validation_classification() {
    let err = AppError::AccountValidation("test".to_string());
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "account.validation_failed");
    assert_eq!(err.category(), "account");
    assert!(!err.retryable());
    assert_eq!(
        err.hint(),
        Some("Run account summary to list available account hashes and nicknames.")
    );
}

// -- schwab::Error variants wrapped in AppError::Schwab ------------------------

#[test]
fn schwab_auth_required_classification() {
    let err = AppError::Schwab(schwab::Error::AuthRequired);
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.code(), "auth.required");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert_eq!(
        err.hint(),
        Some("Run auth refresh, or re-authenticate with auth login-url and auth exchange.")
    );
}

#[test]
fn schwab_auth_expired_classification() {
    let err = AppError::Schwab(schwab::Error::AuthExpired);
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.code(), "auth.expired");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert_eq!(
        err.hint(),
        Some("Run auth refresh, or re-authenticate with auth login-url and auth exchange.")
    );
}

#[test]
fn schwab_auth_callback_classification() {
    let err = AppError::Schwab(schwab::Error::AuthCallback("timeout".into()));
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.code(), "auth.callback_failed");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_http_status_classification() {
    let err = AppError::Schwab(schwab::Error::HttpStatus {
        status: 403,
        body: "forbidden".into(),
    });
    assert_eq!(err.exit_code(), 4);
    assert_eq!(err.code(), "schwab.http_status");
    assert_eq!(err.category(), "schwab");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_decode_classification() {
    let err = AppError::Schwab(schwab::Error::Decode {
        source: json_error(),
        body: "bad json".into(),
    });
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "schwab.decode_failed");
    assert_eq!(err.category(), "schwab");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_json_classification() {
    let err = AppError::Schwab(schwab::Error::Json(json_error()));
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "auth.json_failed");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_io_classification() {
    let err = AppError::Schwab(schwab::Error::Io(std::io::Error::other("test")));
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "auth.io_failed");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_empty_symbols_classification() {
    let err = AppError::Schwab(schwab::Error::EmptySymbols);
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "input.empty_symbols");
    assert_eq!(err.category(), "input");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_missing_required_parameter_classification() {
    let err = AppError::Schwab(schwab::Error::MissingRequiredParameter("symbol"));
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "input.missing_parameter");
    assert_eq!(err.category(), "input");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_invalid_auth_config_classification() {
    let err = AppError::Schwab(schwab::Error::InvalidAuthConfig {
        field: "client_id",
        message: "empty".into(),
    });
    assert_eq!(err.exit_code(), 20);
    assert_eq!(err.code(), "auth.config_invalid");
    assert_eq!(err.category(), "auth");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_empty_base_url_classification() {
    let err = AppError::Schwab(schwab::Error::EmptyBaseUrl);
    assert_eq!(err.exit_code(), 20);
    assert_eq!(err.code(), "config.base_url_invalid");
    assert_eq!(err.category(), "config");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_invalid_base_url_classification() {
    let err = AppError::Schwab(schwab::Error::InvalidBaseUrl {
        base_url: "not-a-url".into(),
        message: "invalid".into(),
    });
    assert_eq!(err.exit_code(), 20);
    assert_eq!(err.code(), "config.base_url_invalid");
    assert_eq!(err.category(), "config");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn schwab_encode_classification() {
    let err = AppError::Schwab(schwab::Error::Encode(json_error()));
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "json.encode_failed");
    assert_eq!(err.category(), "json");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}

#[test]
fn mutable_disabled_classification() {
    let err = AppError::MutableDisabled;
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "config.mutable_disabled");
    assert_eq!(err.category(), "config");
    assert!(!err.retryable());
    assert!(
        err.hint()
            .unwrap()
            .contains("i-also-like-to-live-dangerously")
    );
}

// -- Display messages ----------------------------------------------------------

#[test]
fn display_includes_inner_details() {
    let err = AppError::MissingAuthConfig("client_secret");
    assert!(err.to_string().contains("client_secret"));

    let err = AppError::TokenFileMissing("/tmp/tok.json".into());
    assert!(err.to_string().contains("/tmp/tok.json"));

    let err = AppError::Io(std::io::Error::other("disk"));
    assert!(err.to_string().contains("disk"));

    let err = AppError::Json(json_error());
    assert!(err.to_string().contains("JSON error"));

    let err = AppError::Schwab(schwab::Error::AuthRequired);
    assert!(err.to_string().contains("Schwab error"));
}

#[test]
fn display_options_symbol_not_found() {
    let err = AppError::OptionsSymbolNotFound {
        symbol: "FAKE".to_string(),
    };
    assert!(err.to_string().contains("FAKE"));
}

#[test]
fn display_options_validation() {
    let err = AppError::OptionsValidation {
        message: "bad input".to_string(),
    };
    assert!(err.to_string().contains("bad input"));
}

#[test]
fn display_mutable_disabled() {
    let err = AppError::MutableDisabled;
    assert!(err.to_string().contains("mutable operations"));
}

// -- hint() coverage for wildcard arm ------------------------------------------

#[test]
fn hint_returns_none_for_non_auth_schwab_errors() {
    let err = AppError::Schwab(schwab::Error::EmptySymbols);
    assert!(err.hint().is_none());

    let err = AppError::Schwab(schwab::Error::HttpStatus {
        status: 500,
        body: String::new(),
    });
    assert!(err.hint().is_none());
}

#[test]
fn ta_insufficient_data_classification() {
    let err = AppError::TaInsufficientData {
        needed: 252,
        got: 50,
        indicator: "SMA-200".to_string(),
    };
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "ta.insufficient_data");
    assert_eq!(err.category(), "ta");
    assert!(!err.retryable());
    assert!(err.hint().unwrap().contains("interval"));
}

#[test]
fn ta_invalid_interval_classification() {
    let err = AppError::TaInvalidInterval {
        interval: "hourly".to_string(),
    };
    assert_eq!(err.exit_code(), 10);
    assert_eq!(err.code(), "ta.invalid_interval");
    assert_eq!(err.category(), "ta");
    assert!(!err.retryable());
    assert!(err.hint().is_some());
}

#[test]
fn ta_calculation_error_classification() {
    let err = AppError::TaCalculationError {
        indicator: "stochastic".to_string(),
        reason: "division by zero".to_string(),
    };
    assert_eq!(err.exit_code(), 20);
    assert_eq!(err.code(), "ta.calculation_error");
    assert_eq!(err.category(), "ta");
    assert!(!err.retryable());
    assert!(err.hint().is_none());
}
