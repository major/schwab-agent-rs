use std::fs::File;
use std::path::Path;

use std::time::Duration;

use schwab::auth::{
    AuthConfig, AuthContext, FileTokenStore, Provider, TokenFile, authorize_url,
    exchange_redirect_url, start_login,
};
use serde::Serialize;
use serde_json::{Value, to_value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::cli::{AuthCommand, AuthExchangeArgs, Cli, LoginArgs, LoginUrlArgs};
use crate::error::AppError;

/// Maximum age of a Schwab refresh token in seconds (6.5 days, per Schwab's documented policy).
const REFRESH_TOKEN_MAX_AGE_SECONDS: i64 = 561_600;

/// Dispatches an `auth` subcommand to the appropriate handler.
pub(crate) async fn handle(cli: &Cli, command: &AuthCommand) -> Result<Value, AppError> {
    match command {
        AuthCommand::Status => status(cli),
        AuthCommand::Login(args) => login(cli, args).await,
        AuthCommand::LoginUrl(args) => login_url(cli, args),
        AuthCommand::Exchange(args) => exchange(cli, args).await,
        AuthCommand::Refresh => refresh(cli).await,
    }
}

/// Builds an `AuthConfig` from CLI flags, returning an error if required credentials are absent.
pub(crate) fn build_config(cli: &Cli) -> Result<AuthConfig, AppError> {
    let client_id = cli
        .client_id
        .as_deref()
        .ok_or(AppError::MissingAuthConfig("client_id"))?;
    let client_secret = cli
        .client_secret
        .as_deref()
        .ok_or(AppError::MissingAuthConfig("client_secret"))?;
    Ok(AuthConfig::new(
        client_id,
        client_secret,
        &cli.callback_url,
    )?)
}

/// Returns a `Provider` backed by the saved token file, failing if the file does not exist.
pub(crate) fn provider(cli: &Cli) -> Result<Provider, AppError> {
    let token_path = cli.token_path();
    require_token_file(&token_path)?;
    Ok(Provider::from_token_file(build_config(cli)?, token_path)?)
}

/// Reads the token file and returns a JSON summary of its current auth state.
///
/// Returns a "missing" status object when no token file exists, rather than an error,
/// so callers can inspect the state without special-casing the absent-file case.
fn status(cli: &Cli) -> Result<Value, AppError> {
    let token_path = cli.token_path();
    if !token_path.exists() {
        return Ok(to_value(AuthStatus::missing(&token_path))?);
    }

    let token_file: TokenFile = serde_json::from_reader(File::open(&token_path)?)?;
    Ok(to_value(AuthStatus::from_token_file(
        &token_path,
        &token_file,
    ))?)
}

/// Runs the full interactive OAuth login flow, blocking until Schwab redirects to the callback.
///
/// Optionally opens the authorization URL in the system browser. Writes the resulting
/// token to disk via `FileTokenStore` so subsequent commands can reuse it.
async fn login(cli: &Cli, args: &LoginArgs) -> Result<Value, AppError> {
    let token_path = cli.token_path();
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let session = start_login(build_config(cli)?, FileTokenStore::new(&token_path))?
        .timeout(Some(Duration::from_secs(args.timeout)));
    let url = session.auth_context().authorization_url.clone();
    let browser_opened = if args.no_browser {
        false
    } else {
        open::that(&url).is_ok()
    };
    // Block until Schwab redirects to the local callback server.
    let _provider = session.wait().await?;
    Ok(to_value(LoginOutput {
        logged_in: true,
        token_path: token_path.display().to_string(),
        browser_opened,
    })?)
}

/// Generates the Schwab authorization URL without starting a local callback server.
///
/// Useful for headless or scripted flows where the caller handles the redirect manually
/// and will later call `exchange` with the redirect URL.
fn login_url(cli: &Cli, args: &LoginUrlArgs) -> Result<Value, AppError> {
    let context = authorize_url(&build_config(cli)?)?;
    let browser_opened = if args.no_browser {
        false
    } else {
        open::that(&context.authorization_url).is_ok()
    };
    Ok(to_value(LoginUrlOutput {
        authorization_url: context.authorization_url,
        callback_url: context.callback_url,
        state: context.state,
        token_path: cli.token_path().display().to_string(),
        browser_opened,
    })?)
}

/// Exchanges a Schwab redirect URL for an access/refresh token pair and saves it to disk.
///
/// This is the second step of the manual login flow started by `login_url`.
async fn exchange(cli: &Cli, args: &AuthExchangeArgs) -> Result<Value, AppError> {
    let context = AuthContext {
        callback_url: cli.callback_url.clone(),
        authorization_url: String::new(),
        state: args.state.clone(),
    };
    let token_path = cli.token_path();
    exchange_redirect_url(
        build_config(cli)?,
        FileTokenStore::new(&token_path),
        &context,
        &args.redirect_url,
    )
    .await?;
    Ok(to_value(TokenSavedOutput {
        token_saved: true,
        token_path: token_path.display().to_string(),
    })?)
}

/// Uses the saved refresh token to obtain a new access token and overwrites the token file.
async fn refresh(cli: &Cli) -> Result<Value, AppError> {
    let token_path = cli.token_path();
    let token_file = provider(cli)?.refresh().await?;
    Ok(to_value(RefreshOutput {
        refreshed: true,
        token_path: token_path.display().to_string(),
        access_expires_at: token_file.token.expires_at.and_then(format_epoch),
    })?)
}

/// Returns an error if the token file at `path` does not exist.
///
/// Centralizes the missing-token check so callers don't repeat the same path-exists guard.
fn require_token_file(path: &Path) -> Result<(), AppError> {
    if path.exists() {
        Ok(())
    } else {
        Err(AppError::TokenFileMissing(path.display().to_string()))
    }
}

/// Returns the current UTC time as a Unix timestamp in seconds.
fn now_epoch() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Converts a Unix timestamp in seconds to an RFC 3339 string, returning `None` on overflow.
fn format_epoch(epoch: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(epoch)
        .ok()
        .and_then(|timestamp| timestamp.format(&Rfc3339).ok())
}

/// JSON output for the `auth status` command, describing the current token state.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
struct AuthStatus {
    /// Whether a token file exists on disk.
    token_present: bool,
    /// Absolute path to the token file.
    token_path: String,
    /// RFC 3339 timestamp when the access token expires, if known.
    access_expires_at: Option<String>,
    /// Whether the access token has already expired, if an expiry time is recorded.
    access_expired: Option<bool>,
    /// RFC 3339 timestamp when the refresh token was created (derived from the token file).
    refresh_created_at: Option<String>,
    /// RFC 3339 timestamp when the refresh token will expire, based on `REFRESH_TOKEN_MAX_AGE_SECONDS`.
    refresh_expires_at: Option<String>,
    /// Whether the refresh token has already expired.
    refresh_expired: Option<bool>,
    /// Whether a token refresh can be attempted right now (refresh token present and not expired).
    refresh_possible: bool,
}

impl AuthStatus {
    /// Returns an `AuthStatus` representing the state when no token file exists.
    #[must_use]
    fn missing(token_path: &Path) -> Self {
        Self {
            token_present: false,
            token_path: token_path.display().to_string(),
            access_expires_at: None,
            access_expired: None,
            refresh_created_at: None,
            refresh_expires_at: None,
            refresh_expired: None,
            refresh_possible: false,
        }
    }

    /// Builds an `AuthStatus` by inspecting a loaded `TokenFile` against the current time.
    ///
    /// Refresh token expiry is computed from `token_file.creation_timestamp` plus
    /// `REFRESH_TOKEN_MAX_AGE_SECONDS`, since Schwab does not embed that expiry in the file.
    #[must_use]
    fn from_token_file(token_path: &Path, token_file: &TokenFile) -> Self {
        let now = now_epoch();
        let refresh_expires_at_epoch =
            token_file.creation_timestamp + REFRESH_TOKEN_MAX_AGE_SECONDS;
        Self {
            token_present: true,
            token_path: token_path.display().to_string(),
            access_expires_at: token_file.token.expires_at.and_then(format_epoch),
            access_expired: token_file
                .token
                .expires_at
                .map(|expires_at| expires_at <= now),
            refresh_created_at: format_epoch(token_file.creation_timestamp),
            refresh_expires_at: format_epoch(refresh_expires_at_epoch),
            refresh_expired: Some(refresh_expires_at_epoch <= now),
            refresh_possible: token_file.token.refresh_token.is_some()
                && refresh_expires_at_epoch > now,
        }
    }
}

/// JSON output for the `auth login` command after a successful interactive login.
#[derive(Debug, Serialize)]
struct LoginOutput {
    /// Always `true`; present so callers can confirm success without inspecting exit code.
    logged_in: bool,
    /// Path where the token file was written.
    token_path: String,
    /// Whether the authorization URL was successfully opened in the system browser.
    browser_opened: bool,
}

/// JSON output for the `auth login-url` command, containing everything needed to complete a manual login.
#[derive(Debug, Serialize)]
struct LoginUrlOutput {
    /// The Schwab authorization URL the user must visit to grant access.
    authorization_url: String,
    /// The local callback URL Schwab will redirect to after the user approves.
    callback_url: String,
    /// CSRF state token; must be passed to `auth exchange` to validate the redirect.
    state: String,
    /// Path where the token file will be written after a successful exchange.
    token_path: String,
    /// Whether the authorization URL was successfully opened in the system browser.
    browser_opened: bool,
}

/// JSON output for the `auth exchange` command after the token has been written to disk.
#[derive(Debug, Serialize)]
struct TokenSavedOutput {
    /// Always `true`; present so callers can confirm success without inspecting exit code.
    token_saved: bool,
    /// Path where the token file was written.
    token_path: String,
}

/// JSON output for the `auth refresh` command after a successful token refresh.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
struct RefreshOutput {
    /// Always `true`; present so callers can confirm success without inspecting exit code.
    refreshed: bool,
    /// Path to the token file that was updated.
    token_path: String,
    /// RFC 3339 timestamp when the new access token expires, if the API returned one.
    access_expires_at: Option<String>,
}

#[cfg(test)]
mod tests;
