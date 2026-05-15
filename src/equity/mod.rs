//! Equity (stock) order porcelain commands.
//!
//! Provides build, preview, and place workflows for four equity actions:
//! buy, sell, sell-short, and buy-to-cover. Each action hardcodes the trade
//! instruction so that an LLM (or human) cannot accidentally reverse a trade.
//!
//! ## Workflow
//!
//! Recommended LLM flow:
//!
//! 1. `stock preview <action> --account HASH --save-preview` calls the
//!    Schwab preview API and stores the exact previewed payload.
//! 2. `stock place-from-preview --account HASH --digest HEX` places the saved
//!    preview after the digest, TTL, and account checks pass.
//!
//! `stock build <action>` is a local dry run. `stock place <action>` bypasses
//! the preview flow for direct placement.
//!
//! ## Raw commands
//!
//! `stock preview-raw --json '...'` and `stock place-raw --json '...'` accept
//! arbitrary JSON payloads for complex order types (brackets, OCO, triggers)
//! that don't have dedicated porcelain.

#[cfg(test)]
mod tests;

use clap::{Args, Subcommand, ValueEnum};
use serde_json::Value;

use crate::account;
use crate::auth;
use crate::cli::Cli;
use crate::error::AppError;
use crate::order::preview;

use crate::shared::{DurationChoice, SessionChoice, to_number};
use crate::verify;

// ---------------------------------------------------------------------------
// Equity action enum (hardcoded instructions)
// ---------------------------------------------------------------------------

/// Equity trade action.
///
/// Each variant maps to exactly one [`schwab::Instruction`], preventing
/// accidental trade reversal.
#[derive(Debug, Subcommand)]
pub enum EquityAction {
    /// Buy shares (opens or adds to a long position).
    Buy(EquityArgs),

    /// Sell shares (closes or reduces a long position).
    Sell(EquityArgs),

    /// Sell short (opens a short position by borrowing shares).
    #[command(name = "sell-short")]
    SellShort(EquityArgs),

    /// Buy to cover (closes a short position).
    #[command(name = "buy-to-cover")]
    BuyToCover(EquityArgs),
}

/// Order type selection for equity orders.
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OrderTypeChoice {
    /// Market order: execute immediately at the current market price.
    #[default]
    Market,
    /// Limit order: execute at a specified price or better.
    Limit,
    /// Stop order: trigger a market order when the stop price is reached.
    Stop,
    /// Stop-limit order: trigger a limit order when the stop price is reached.
    #[value(name = "stop-limit")]
    StopLimit,
}

/// Arguments common to all equity actions.
#[derive(Debug, Args)]
pub struct EquityArgs {
    /// Ticker symbol (e.g. AAPL, SPY, TSLA).
    pub symbol: String,

    /// Number of shares.
    #[arg(long, default_value = "1")]
    pub quantity: u32,

    /// Order type (market, limit, stop, stop-limit).
    #[arg(long, value_enum, default_value = "market")]
    pub order_type: OrderTypeChoice,

    /// Limit price (required for limit and stop-limit orders).
    #[arg(long)]
    pub price: Option<f64>,

    /// Stop trigger price (required for stop and stop-limit orders).
    #[arg(long)]
    pub stop_price: Option<f64>,

    /// Trading session.
    #[arg(long, value_enum, default_value = "normal")]
    pub session: SessionChoice,

    /// Time-in-force.
    #[arg(long, value_enum, default_value = "day")]
    pub duration: DurationChoice,
}

// ---------------------------------------------------------------------------
// Command enum
// ---------------------------------------------------------------------------

/// Equity (stock) order construction, preview, and placement workflows.
#[derive(Debug, Subcommand)]
pub enum EquityCommand {
    /// Construct the order JSON locally without calling the API.
    ///
    /// Useful for inspecting the exact payload before previewing or placing.
    #[command(subcommand)]
    Build(EquityAction),

    /// Preview the order via the Schwab API (no funds committed).
    ///
    /// Returns estimated commissions, fees, and validation results.
    /// Add --save-preview to persist the payload as a tamper-evident digest
    /// that can later be placed with `stock place-from-preview`.
    Preview(EquityPreviewArgs),

    /// Place the order directly via the Schwab API.
    ///
    /// Builds the order from the action arguments and submits it.
    /// The order will be live immediately upon acceptance.
    Place(EquityPlaceArgs),

    /// Place an order from a previously saved preview digest.
    ///
    /// Loads the exact order payload that was previewed, verifies the
    /// digest for tamper detection, checks the 15-minute TTL, and submits
    /// the order to the Schwab API.
    #[command(name = "place-from-preview")]
    PlaceFromPreview(PlaceFromPreviewArgs),

    /// Preview an arbitrary JSON order payload (for brackets, OCO, etc.).
    ///
    /// Use this for complex order structures that don't have dedicated
    /// porcelain commands. The JSON is forwarded directly to the Schwab
    /// preview API.
    #[command(name = "preview-raw")]
    PreviewRaw(RawPreviewArgs),

    /// Place an arbitrary JSON order payload (for brackets, OCO, etc.).
    ///
    /// Use this for complex order structures that don't have dedicated
    /// porcelain commands. The JSON is forwarded directly to the Schwab
    /// order API.
    #[command(name = "place-raw")]
    PlaceRaw(RawPlaceArgs),
}

/// Arguments for `stock preview <action>`.
#[derive(Debug, Args)]
pub struct EquityPreviewArgs {
    /// Account hash or nickname (use `account summary` to list accounts).
    #[arg(long)]
    pub account: String,

    /// Save the preview payload as a tamper-evident digest for later placement.
    #[arg(long)]
    pub save_preview: bool,

    /// The action to preview.
    #[command(subcommand)]
    pub action: EquityAction,
}

/// Arguments for `stock place <action>`.
#[derive(Debug, Args)]
pub struct EquityPlaceArgs {
    /// Account hash or nickname (use `account summary` to list accounts).
    #[arg(long)]
    pub account: String,

    /// The action to place.
    #[command(subcommand)]
    pub action: EquityAction,
}

/// Arguments for `stock place-from-preview`.
#[derive(Debug, Args)]
pub struct PlaceFromPreviewArgs {
    /// Account hash or nickname (must resolve to the hash used during preview).
    #[arg(long)]
    pub account: String,

    /// SHA-256 digest from a previous `stock preview --save-preview` run.
    #[arg(long)]
    pub digest: String,
}

/// Arguments for `stock preview-raw`.
#[derive(Debug, Args)]
pub struct RawPreviewArgs {
    /// Account hash or nickname (use `account summary` to list accounts).
    #[arg(long)]
    pub account: String,

    /// Save the preview payload as a tamper-evident digest for later placement.
    #[arg(long)]
    pub save_preview: bool,

    /// Complete order JSON payload (use for brackets, OCO, triggers, etc.).
    #[arg(long)]
    pub json: String,
}

/// Arguments for `stock place-raw`.
#[derive(Debug, Args)]
pub struct RawPlaceArgs {
    /// Account hash or nickname (use `account summary` to list accounts).
    #[arg(long)]
    pub account: String,

    /// Complete order JSON payload (use for brackets, OCO, triggers, etc.).
    #[arg(long)]
    pub json: String,
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

/// Handles all stock subcommands and returns a complete JSON envelope.
///
/// Stock commands bypass the generic envelope wrapping in `execute()` because
/// they compute their own dynamic command names (e.g., `stock.build.buy`).
pub(crate) async fn handle(cli: &Cli, command: &EquityCommand) -> Result<Value, AppError> {
    match command {
        EquityCommand::Build(action) => do_build(action),
        EquityCommand::Preview(args) => {
            let name = format!("stock.preview.{}", action_name(&args.action));
            do_preview(cli, &args.account, &args.action, args.save_preview, &name).await
        }
        EquityCommand::Place(args) => {
            crate::config::require_mutable_enabled()?;
            do_place(cli, &args.account, &args.action).await
        }
        EquityCommand::PlaceFromPreview(args) => {
            crate::config::require_mutable_enabled()?;
            do_place_from_preview(cli, &args.account, &args.digest).await
        }
        EquityCommand::PreviewRaw(args) => {
            do_preview_raw(cli, &args.account, &args.json, args.save_preview).await
        }
        EquityCommand::PlaceRaw(args) => {
            crate::config::require_mutable_enabled()?;
            do_place_raw(cli, &args.account, &args.json).await
        }
    }
}

/// Returns the kebab-case action name for use in command names.
#[must_use]
fn action_name(action: &EquityAction) -> &'static str {
    match action {
        EquityAction::Buy(_) => "buy",
        EquityAction::Sell(_) => "sell",
        EquityAction::SellShort(_) => "sell-short",
        EquityAction::BuyToCover(_) => "buy-to-cover",
    }
}

// ---------------------------------------------------------------------------
// Order construction
// ---------------------------------------------------------------------------

/// Returns the hardcoded [`schwab::Instruction`] for an equity action.
fn instruction(action: &EquityAction) -> schwab::Instruction {
    match action {
        EquityAction::Buy(_) => schwab::Instruction::Buy,
        EquityAction::Sell(_) => schwab::Instruction::Sell,
        EquityAction::SellShort(_) => schwab::Instruction::SellShort,
        EquityAction::BuyToCover(_) => schwab::Instruction::BuyToCover,
    }
}

/// Extracts the shared [`EquityArgs`] from any action variant.
fn action_args(action: &EquityAction) -> &EquityArgs {
    match action {
        EquityAction::Buy(a)
        | EquityAction::Sell(a)
        | EquityAction::SellShort(a)
        | EquityAction::BuyToCover(a) => a,
    }
}

/// Builds a [`schwab::OrderBuilder`] from an equity action.
///
/// Validates that required price fields are present for the selected order
/// type and converts CLI `f64` values to [`schwab::Number`].
fn build_equity_order(action: &EquityAction) -> Result<schwab::OrderBuilder, AppError> {
    let a = action_args(action);
    let inst = instruction(action);
    let qty = to_number(f64::from(a.quantity))?;

    let order = match a.order_type {
        OrderTypeChoice::Market => schwab::OrderBuilder::equity_market(&a.symbol, inst, qty),
        OrderTypeChoice::Limit => {
            let price = a.price.ok_or(AppError::OrderValidation(
                "--price is required for limit orders".to_string(),
            ))?;
            schwab::OrderBuilder::equity_limit(&a.symbol, inst, qty, to_number(price)?)
        }
        OrderTypeChoice::Stop => {
            let stop = a.stop_price.ok_or(AppError::OrderValidation(
                "--stop-price is required for stop orders".to_string(),
            ))?;
            schwab::OrderBuilder::equity_stop(&a.symbol, inst, qty, to_number(stop)?)
        }
        OrderTypeChoice::StopLimit => {
            let price = a.price.ok_or(AppError::OrderValidation(
                "--price is required for stop-limit orders".to_string(),
            ))?;
            let stop = a.stop_price.ok_or(AppError::OrderValidation(
                "--stop-price is required for stop-limit orders".to_string(),
            ))?;
            schwab::OrderBuilder::equity_stop_limit(
                &a.symbol,
                inst,
                qty,
                to_number(price)?,
                to_number(stop)?,
            )
        }
    };

    Ok(order.session(a.session.into()).duration(a.duration.into()))
}

/// Serializes an [`schwab::OrderBuilder`] to a JSON [`Value`].
fn serialize_order(order: &schwab::OrderBuilder) -> Result<Value, AppError> {
    serde_json::to_value(order)
        .map_err(|e| AppError::OrderValidation(format!("failed to serialize order: {e}")))
}

// ---------------------------------------------------------------------------
// Build / Preview / Place handlers
// ---------------------------------------------------------------------------

/// Builds the order JSON locally without any API call.
fn do_build(action: &EquityAction) -> Result<Value, AppError> {
    let order = build_equity_order(action)?;
    serialize_order(&order)
}

/// Previews the order via the Schwab API.
async fn do_preview(
    cli: &Cli,
    account: &str,
    action: &EquityAction,
    save: bool,
    command_name: &str,
) -> Result<Value, AppError> {
    let order = build_equity_order(action)?;
    let client = auth::provider(cli)?.client().await?;
    let resolved = account::resolve_account(&client, account).await?;
    let account_hash = resolved.account_hash;
    let _preview = client.preview_order(&account_hash, &order).await?;

    let order_json = serialize_order(&order)?;

    let mut data = serde_json::json!({
        "order": order_json,
        "preview": "accepted",
    });

    if save {
        let digest = preview::save_preview(&account_hash, &order, command_name)?;
        data["digest"] = Value::String(digest);
        data["digest_ttl_seconds"] = Value::Number(900.into());
    }

    Ok(data)
}

/// Places the order directly via the Schwab API with post-place verification.
async fn do_place(cli: &Cli, account: &str, action: &EquityAction) -> Result<Value, AppError> {
    let order = build_equity_order(action)?;
    let client = auth::provider(cli)?.client().await?;
    let resolved = account::resolve_account(&client, account).await?;
    let account_hash = resolved.account_hash;
    let response = client.place_order(&account_hash, &order).await?;
    let order_json = serialize_order(&order)?;

    let result = verify::verify_order(
        &client,
        &account_hash,
        response.order_id,
        "place",
        response.location,
        Some(order_json),
    )
    .await;

    verify::action_value(result)
}

/// Places an order from a previously saved preview digest with post-place
/// verification.
async fn do_place_from_preview(cli: &Cli, account: &str, digest: &str) -> Result<Value, AppError> {
    let client = auth::provider(cli)?.client().await?;
    let resolved = account::resolve_account(&client, account).await?;
    let account_hash = resolved.account_hash;
    let saved = preview::load_preview(digest, &account_hash)?;
    let response = client.place_order(&account_hash, &saved.order).await?;

    let mut result = verify::verify_order(
        &client,
        &account_hash,
        response.order_id,
        "place",
        response.location,
        Some(saved.order),
    )
    .await;

    result.digest = Some(digest.to_string());
    result.original_command = Some(saved.command);

    verify::action_value(result)
}

// ---------------------------------------------------------------------------
// Raw JSON handlers (brackets, OCO, triggers)
// ---------------------------------------------------------------------------

/// Parses a raw JSON string into a [`Value`].
fn parse_raw_json(json: &str) -> Result<Value, AppError> {
    serde_json::from_str(json)
        .map_err(|e| AppError::OrderValidation(format!("invalid JSON payload: {e}")))
}

/// Previews a raw JSON order payload via the Schwab API.
async fn do_preview_raw(
    cli: &Cli,
    account: &str,
    json: &str,
    save: bool,
) -> Result<Value, AppError> {
    let order = parse_raw_json(json)?;
    let client = auth::provider(cli)?.client().await?;
    let resolved = account::resolve_account(&client, account).await?;
    let account_hash = resolved.account_hash;
    let _preview = client.preview_order(&account_hash, &order).await?;

    let mut data = serde_json::json!({
        "order": order,
        "preview": "accepted",
    });

    if save {
        let digest = preview::save_preview(&account_hash, &order, "stock.preview-raw")?;
        data["digest"] = Value::String(digest);
        data["digest_ttl_seconds"] = Value::Number(900.into());
    }

    Ok(data)
}

/// Places a raw JSON order payload via the Schwab API with post-place
/// verification.
async fn do_place_raw(cli: &Cli, account: &str, json: &str) -> Result<Value, AppError> {
    let order = parse_raw_json(json)?;
    let client = auth::provider(cli)?.client().await?;
    let resolved = account::resolve_account(&client, account).await?;
    let account_hash = resolved.account_hash;
    let response = client.place_order(&account_hash, &order).await?;

    let result = verify::verify_order(
        &client,
        &account_hash,
        response.order_id,
        "place",
        response.location,
        Some(order),
    )
    .await;

    verify::action_value(result)
}
