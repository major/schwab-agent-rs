//! Option order porcelain commands.
//!
//! Provides build, preview, place, and replace workflows for 15 named option strategies.
//! Every porcelain command hardcodes the contract type and direction so that an
//! LLM (or human) cannot accidentally reverse a trade. Strike parameters use
//! factual names (`--high-strike`, `--low-strike`) rather than directional names
//! to eliminate ambiguity.
//!
//! ## Workflow
//!
//! Recommended LLM flow:
//!
//! 1. `order preview <strategy> --account HASH --save-preview` calls the
//!    Schwab preview API and stores the exact previewed payload.
//! 2. `order place-from-preview --account HASH --digest HEX` places the saved
//!    preview after the digest, TTL, and account checks pass.
//!
//! `order build <strategy>` remains available as a local dry run. `order place
//! <strategy> --account HASH` remains available for direct placement when a
//! human explicitly wants to bypass the saved-preview flow.
//! Agents should include `--price` when practical so orders use limit,
//! net-debit, or net-credit pricing. Omitting `--price` intentionally creates a
//! market order.

pub(crate) mod builder;
pub(crate) mod lifecycle;
pub(crate) mod preview;

use clap::{Args, Subcommand};
use serde_json::Value;

use crate::account;
use crate::auth;
use crate::cli::Cli;
use crate::error::AppError;
use crate::output::{CommandOutput, Envelope, Metadata};
use crate::shared::{DurationChoice, SessionChoice};
use crate::verify;

use builder::OptionOrder;

// ---------------------------------------------------------------------------
// Strategy argument structs
// ---------------------------------------------------------------------------

/// Arguments for single-leg option strategies.
///
/// Used by: long-call, long-put, cash-secured-put, naked-call, sell-covered-call.
#[derive(Debug, Args)]
pub struct SingleLegArgs {
    /// Underlying ticker symbol (e.g. AAPL, SPY, TSLA).
    pub underlying: String,

    /// Option expiration date in YYYY-MM-DD format (e.g. 2025-06-20).
    #[arg(long)]
    pub expiration: String,

    /// Strike price (e.g. 200.00).
    #[arg(long)]
    pub strike: f64,

    /// Number of contracts. Each contract represents 100 shares.
    #[arg(long, default_value = "1")]
    pub quantity: u32,

    /// Limit price per contract. Preferred for agents; omit only for a deliberate market order.
    #[arg(long)]
    pub price: Option<f64>,

    /// Trading session.
    #[arg(long, value_enum, default_value = "normal")]
    pub session: SessionChoice,

    /// Order duration (time-in-force).
    #[arg(long, value_enum, default_value = "day")]
    pub duration: DurationChoice,
}

/// Arguments for vertical spread strategies.
///
/// Used by: put-credit-spread, call-credit-spread, put-debit-spread,
/// call-debit-spread. Strike names are factual (high/low), not directional,
/// to prevent confusion.
#[derive(Debug, Args)]
pub struct VerticalArgs {
    /// Underlying ticker symbol (e.g. AAPL, SPY, TSLA).
    pub underlying: String,

    /// Option expiration date in YYYY-MM-DD format (e.g. 2025-06-20).
    #[arg(long)]
    pub expiration: String,

    /// Higher strike price. Must be greater than --low-strike.
    #[arg(long)]
    pub high_strike: f64,

    /// Lower strike price. Must be less than --high-strike.
    #[arg(long)]
    pub low_strike: f64,

    /// Number of contracts (applies to both legs). Each contract = 100 shares.
    #[arg(long, default_value = "1")]
    pub quantity: u32,

    /// Net debit or credit limit price. Preferred for agents; omit only for a deliberate market order.
    #[arg(long)]
    pub price: Option<f64>,

    /// Trading session.
    #[arg(long, value_enum, default_value = "normal")]
    pub session: SessionChoice,

    /// Order duration (time-in-force).
    #[arg(long, value_enum, default_value = "day")]
    pub duration: DurationChoice,
}

/// Arguments for straddle strategies (same strike for call and put).
///
/// Used by: long-straddle, short-straddle.
#[derive(Debug, Args)]
pub struct StraddleArgs {
    /// Underlying ticker symbol (e.g. AAPL, SPY, TSLA).
    pub underlying: String,

    /// Option expiration date in YYYY-MM-DD format (e.g. 2025-06-20).
    #[arg(long)]
    pub expiration: String,

    /// Strike price for both the call and put legs.
    #[arg(long)]
    pub strike: f64,

    /// Number of contracts (applies to both legs). Each contract = 100 shares.
    #[arg(long, default_value = "1")]
    pub quantity: u32,

    /// Net debit or credit limit price. Preferred for agents; omit only for a deliberate market order.
    #[arg(long)]
    pub price: Option<f64>,

    /// Trading session.
    #[arg(long, value_enum, default_value = "normal")]
    pub session: SessionChoice,

    /// Order duration (time-in-force).
    #[arg(long, value_enum, default_value = "day")]
    pub duration: DurationChoice,
}

/// Arguments for strangle strategies (different strikes for call and put).
///
/// Used by: long-strangle, short-strangle.
#[derive(Debug, Args)]
pub struct StrangleArgs {
    /// Underlying ticker symbol (e.g. AAPL, SPY, TSLA).
    pub underlying: String,

    /// Option expiration date in YYYY-MM-DD format (e.g. 2025-06-20).
    #[arg(long)]
    pub expiration: String,

    /// Strike price for the call leg (typically above the current price).
    #[arg(long)]
    pub call_strike: f64,

    /// Strike price for the put leg (typically below the current price).
    #[arg(long)]
    pub put_strike: f64,

    /// Number of contracts (applies to both legs). Each contract = 100 shares.
    #[arg(long, default_value = "1")]
    pub quantity: u32,

    /// Net debit or credit limit price. Preferred for agents; omit only for a deliberate market order.
    #[arg(long)]
    pub price: Option<f64>,

    /// Trading session.
    #[arg(long, value_enum, default_value = "normal")]
    pub session: SessionChoice,

    /// Order duration (time-in-force).
    #[arg(long, value_enum, default_value = "day")]
    pub duration: DurationChoice,
}

/// Arguments for the short iron condor strategy (four legs).
#[derive(Debug, Args)]
pub struct IronCondorArgs {
    /// Underlying ticker symbol (e.g. AAPL, SPY, TSLA).
    pub underlying: String,

    /// Option expiration date in YYYY-MM-DD format (e.g. 2025-06-20).
    #[arg(long)]
    pub expiration: String,

    /// Lowest strike: the protective put you BUY (defines max loss on downside).
    #[arg(long)]
    pub put_long_strike: f64,

    /// Second lowest strike: the put you SELL (collects premium).
    #[arg(long)]
    pub put_short_strike: f64,

    /// Second highest strike: the call you SELL (collects premium).
    #[arg(long)]
    pub call_short_strike: f64,

    /// Highest strike: the protective call you BUY (defines max loss on upside).
    #[arg(long)]
    pub call_long_strike: f64,

    /// Number of contracts (applies to all four legs). Each contract = 100 shares.
    #[arg(long, default_value = "1")]
    pub quantity: u32,

    /// Net credit limit price for the condor. Preferred for agents; omit only for a deliberate market order.
    #[arg(long)]
    pub price: Option<f64>,

    /// Trading session.
    #[arg(long, value_enum, default_value = "normal")]
    pub session: SessionChoice,

    /// Order duration (time-in-force).
    #[arg(long, value_enum, default_value = "day")]
    pub duration: DurationChoice,
}

/// Arguments for the jade lizard strategy (three legs).
#[derive(Debug, Args)]
pub struct JadeLizardArgs {
    /// Underlying ticker symbol (e.g. AAPL, SPY, TSLA).
    pub underlying: String,

    /// Option expiration date in YYYY-MM-DD format (e.g. 2025-06-20).
    #[arg(long)]
    pub expiration: String,

    /// Put strike price. This put is SOLD to collect premium.
    #[arg(long)]
    pub put_strike: f64,

    /// Lower call strike. This call is SOLD to collect premium.
    #[arg(long)]
    pub short_call_strike: f64,

    /// Higher call strike. This call is BOUGHT for upside protection.
    /// Must be greater than --short-call-strike.
    #[arg(long)]
    pub long_call_strike: f64,

    /// Number of contracts (applies to all three legs). Each contract = 100 shares.
    #[arg(long, default_value = "1")]
    pub quantity: u32,

    /// Net credit limit price for the trade. Preferred for agents; omit only for a deliberate market order.
    #[arg(long)]
    pub price: Option<f64>,

    /// Trading session.
    #[arg(long, value_enum, default_value = "normal")]
    pub session: SessionChoice,

    /// Order duration (time-in-force).
    #[arg(long, value_enum, default_value = "day")]
    pub duration: DurationChoice,
}

// ---------------------------------------------------------------------------
// Strategy command enum (15 porcelain commands)
// ---------------------------------------------------------------------------

/// Named option strategies with hardcoded directions.
///
/// Each variant locks the contract type and buy/sell direction so that an
/// LLM or script cannot accidentally reverse a trade.
#[derive(Debug, Subcommand)]
pub enum StrategyCommand {
    /// Buy a call option (bullish, limited risk).
    ///
    /// Hardcoded: contract=CALL, direction=BUY_TO_OPEN.
    /// You pay a premium for the right to buy shares at the strike price.
    /// Max loss: premium paid. Max gain: unlimited.
    /// Profit when: underlying rises above strike + premium paid.
    #[command(long_about = "Buy a call option (bullish, limited risk).\n\n\
        HARDCODED: contract=CALL, direction=BUY_TO_OPEN.\n\
        You pay a premium for the right to buy 100 shares per contract at the\n\
        strike price before expiration.\n\n\
        Max loss: premium paid.\n\
        Max gain: unlimited (underlying can rise indefinitely).\n\
        Profit when: underlying rises above strike + premium paid.\n\n\
        Example:\n  \
        schwab-agent order build long-call AAPL --expiration 2025-06-20 --strike 200 --price 5.50")]
    LongCall(SingleLegArgs),

    /// Buy a put option (bearish, limited risk).
    ///
    /// Hardcoded: contract=PUT, direction=BUY_TO_OPEN.
    /// You pay a premium for the right to sell shares at the strike price.
    /// Max loss: premium paid. Max gain: strike - premium (underlying to zero).
    /// Profit when: underlying falls below strike - premium paid.
    #[command(long_about = "Buy a put option (bearish, limited risk).\n\n\
        HARDCODED: contract=PUT, direction=BUY_TO_OPEN.\n\
        You pay a premium for the right to sell 100 shares per contract at the\n\
        strike price before expiration.\n\n\
        Max loss: premium paid.\n\
        Max gain: strike price minus premium (if underlying goes to zero).\n\
        Profit when: underlying falls below strike - premium paid.\n\n\
        Example:\n  \
        schwab-agent order build long-put AAPL --expiration 2025-06-20 --strike 180 --price 3.00")]
    LongPut(SingleLegArgs),

    /// Sell a put secured by cash to cover potential assignment (neutral-bullish).
    ///
    /// Hardcoded: contract=PUT, direction=SELL_TO_OPEN.
    /// You collect premium but must buy shares if assigned. Requires cash
    /// equal to strike x 100 x quantity.
    /// Max loss: (strike - premium) x 100 per contract.
    /// Max gain: premium received.
    #[command(
        name = "cash-secured-put",
        long_about = "Sell a put option secured by cash (neutral to bullish).\n\n\
            HARDCODED: contract=PUT, direction=SELL_TO_OPEN.\n\
            You collect premium and accept the obligation to buy 100 shares per\n\
            contract at the strike price if assigned. Your account must hold\n\
            enough cash to cover assignment (strike x 100 x quantity).\n\n\
            Max loss: (strike - premium received) x 100 per contract.\n\
            Max gain: premium received.\n\
            Profit when: underlying stays above strike through expiration.\n\n\
            Example:\n  \
            schwab-agent order build cash-secured-put AAPL --expiration 2025-06-20 --strike 170 --price 2.50"
    )]
    CashSecuredPut(SingleLegArgs),

    /// Sell a naked call without owning the underlying (bearish, UNLIMITED RISK).
    ///
    /// Hardcoded: contract=CALL, direction=SELL_TO_OPEN.
    /// You collect premium but face unlimited loss if the underlying rises.
    /// Max loss: UNLIMITED. Max gain: premium received.
    #[command(
        name = "naked-call",
        long_about = "Sell a naked (uncovered) call option (bearish, UNLIMITED RISK).\n\n\
            HARDCODED: contract=CALL, direction=SELL_TO_OPEN.\n\
            You collect premium and accept the obligation to sell 100 shares per\n\
            contract at the strike price if assigned. You do NOT own the shares.\n\n\
            WARNING: MAX LOSS IS UNLIMITED. The underlying can rise indefinitely.\n\
            Max gain: premium received.\n\
            Profit when: underlying stays below strike through expiration.\n\n\
            Example:\n  \
            schwab-agent order build naked-call AAPL --expiration 2025-06-20 --strike 220 --price 1.50"
    )]
    NakedCall(SingleLegArgs),

    /// Sell a call against shares you already own (neutral, capped upside).
    ///
    /// Hardcoded: contract=CALL, direction=SELL_TO_OPEN.
    /// You collect premium on shares you hold. Your upside is capped at the
    /// strike price.
    #[command(
        name = "sell-covered-call",
        long_about = "Sell a covered call against shares you already own.\n\n\
            HARDCODED: contract=CALL, direction=SELL_TO_OPEN.\n\
            You collect premium on 100 shares per contract that you already hold.\n\
            If assigned, you sell your shares at the strike price. Your upside\n\
            on the shares is capped at the strike.\n\n\
            Max loss: underlying drops to zero minus premium received (same as holding shares).\n\
            Max gain: (strike - cost basis + premium) x 100 per contract.\n\
            Profit when: underlying stays below strike through expiration (keep premium + shares).\n\n\
            Example:\n  \
            schwab-agent order build sell-covered-call AAPL --expiration 2025-06-20 --strike 210 --price 4.00"
    )]
    SellCoveredCall(SingleLegArgs),

    /// Bull put spread: sell high put, buy low put for net credit (bullish).
    ///
    /// Hardcoded: BUY_TO_OPEN the low-strike put, SELL_TO_OPEN the high-strike
    /// put. Same expiration and contract type (PUT). Net credit received.
    #[command(
        name = "put-credit-spread",
        long_about = "Bull put spread: sell a higher-strike put, buy a lower-strike put (bullish).\n\n\
            HARDCODED: contract=PUT for both legs.\n\
            - SELL_TO_OPEN the high-strike put (collects premium).\n\
            - BUY_TO_OPEN the low-strike put (limits downside).\n\
            Order type: NET_CREDIT.\n\n\
            Max loss: (high_strike - low_strike - credit received) x 100.\n\
            Max gain: credit received x 100.\n\
            Profit when: underlying stays above high strike through expiration.\n\n\
            Example:\n  \
            schwab-agent order build put-credit-spread AAPL --expiration 2025-06-20 \\\n    \
            --high-strike 200 --low-strike 190 --price 3.00"
    )]
    PutCreditSpread(VerticalArgs),

    /// Bear call spread: sell low call, buy high call for net credit (bearish).
    ///
    /// Hardcoded: BUY_TO_OPEN the high-strike call, SELL_TO_OPEN the low-strike
    /// call. Same expiration and contract type (CALL). Net credit received.
    #[command(
        name = "call-credit-spread",
        long_about = "Bear call spread: sell a lower-strike call, buy a higher-strike call (bearish).\n\n\
            HARDCODED: contract=CALL for both legs.\n\
            - SELL_TO_OPEN the low-strike call (collects premium).\n\
            - BUY_TO_OPEN the high-strike call (limits upside risk).\n\
            Order type: NET_CREDIT.\n\n\
            Max loss: (high_strike - low_strike - credit received) x 100.\n\
            Max gain: credit received x 100.\n\
            Profit when: underlying stays below low strike through expiration.\n\n\
            Example:\n  \
            schwab-agent order build call-credit-spread AAPL --expiration 2025-06-20 \\\n    \
            --high-strike 220 --low-strike 210 --price 2.50"
    )]
    CallCreditSpread(VerticalArgs),

    /// Bear put spread: buy high put, sell low put for net debit (bearish).
    ///
    /// Hardcoded: BUY_TO_OPEN the high-strike put, SELL_TO_OPEN the low-strike
    /// put. Same expiration and contract type (PUT). Net debit paid.
    #[command(
        name = "put-debit-spread",
        long_about = "Bear put spread: buy a higher-strike put, sell a lower-strike put (bearish).\n\n\
            HARDCODED: contract=PUT for both legs.\n\
            - BUY_TO_OPEN the high-strike put (profits from decline).\n\
            - SELL_TO_OPEN the low-strike put (reduces cost).\n\
            Order type: NET_DEBIT.\n\n\
            Max loss: debit paid x 100.\n\
            Max gain: (high_strike - low_strike - debit paid) x 100.\n\
            Profit when: underlying falls below low strike by expiration.\n\n\
            Example:\n  \
            schwab-agent order build put-debit-spread AAPL --expiration 2025-06-20 \\\n    \
            --high-strike 200 --low-strike 190 --price 4.00"
    )]
    PutDebitSpread(VerticalArgs),

    /// Bull call spread: buy low call, sell high call for net debit (bullish).
    ///
    /// Hardcoded: BUY_TO_OPEN the low-strike call, SELL_TO_OPEN the high-strike
    /// call. Same expiration and contract type (CALL). Net debit paid.
    #[command(
        name = "call-debit-spread",
        long_about = "Bull call spread: buy a lower-strike call, sell a higher-strike call (bullish).\n\n\
            HARDCODED: contract=CALL for both legs.\n\
            - BUY_TO_OPEN the low-strike call (profits from rally).\n\
            - SELL_TO_OPEN the high-strike call (reduces cost, caps gain).\n\
            Order type: NET_DEBIT.\n\n\
            Max loss: debit paid x 100.\n\
            Max gain: (high_strike - low_strike - debit paid) x 100.\n\
            Profit when: underlying rises above high strike by expiration.\n\n\
            Example:\n  \
            schwab-agent order build call-debit-spread AAPL --expiration 2025-06-20 \\\n    \
            --high-strike 210 --low-strike 200 --price 4.50"
    )]
    CallDebitSpread(VerticalArgs),

    /// Buy a call and put at the same strike (neutral, profit from big moves).
    ///
    /// Hardcoded: BUY_TO_OPEN both legs. Net debit paid.
    /// Profits from large moves in either direction.
    #[command(
        name = "long-straddle",
        long_about = "Buy a call and a put at the same strike and expiration (volatility bet).\n\n\
            HARDCODED: direction=BUY_TO_OPEN for both legs.\n\
            Order type: NET_DEBIT.\n\n\
            Max loss: total premium paid (both legs).\n\
            Max gain: unlimited on upside, strike minus premium on downside.\n\
            Profit when: underlying moves significantly in EITHER direction\n\
            beyond the total premium paid.\n\n\
            Example:\n  \
            schwab-agent order build long-straddle AAPL --expiration 2025-06-20 --strike 200 --price 12.00"
    )]
    LongStraddle(StraddleArgs),

    /// Sell a call and put at the same strike (neutral, profit from low volatility).
    ///
    /// Hardcoded: SELL_TO_OPEN both legs. Net credit received.
    /// SIGNIFICANT RISK from large moves in either direction.
    #[command(
        name = "short-straddle",
        long_about = "Sell a call and a put at the same strike and expiration (volatility sale).\n\n\
            HARDCODED: direction=SELL_TO_OPEN for both legs.\n\
            Order type: NET_CREDIT.\n\n\
            WARNING: SIGNIFICANT RISK from large moves in either direction.\n\
            Max loss: unlimited on upside, (strike - premium) on downside.\n\
            Max gain: total premium received.\n\
            Profit when: underlying stays near the strike through expiration.\n\n\
            Example:\n  \
            schwab-agent order build short-straddle AAPL --expiration 2025-06-20 --strike 200 --price 12.00"
    )]
    ShortStraddle(StraddleArgs),

    /// Buy a call and put at different strikes (neutral, profit from big moves).
    ///
    /// Hardcoded: BUY_TO_OPEN both legs. Net debit paid.
    /// Cheaper than a straddle but needs a larger move to profit.
    #[command(
        name = "long-strangle",
        long_about = "Buy a call and a put at different strikes, same expiration (volatility bet).\n\n\
            HARDCODED: direction=BUY_TO_OPEN for both legs.\n\
            Order type: NET_DEBIT.\n\
            The call strike is typically above and the put strike below the\n\
            current underlying price.\n\n\
            Max loss: total premium paid.\n\
            Max gain: unlimited on upside, put strike minus premium on downside.\n\
            Profit when: underlying moves beyond either strike by more than\n\
            the total premium paid.\n\n\
            Example:\n  \
            schwab-agent order build long-strangle AAPL --expiration 2025-06-20 \\\n    \
            --call-strike 210 --put-strike 190 --price 6.00"
    )]
    LongStrangle(StrangleArgs),

    /// Sell a call and put at different strikes (neutral, profit from low volatility).
    ///
    /// Hardcoded: SELL_TO_OPEN both legs. Net credit received.
    /// SIGNIFICANT RISK from large moves beyond either strike.
    #[command(
        name = "short-strangle",
        long_about = "Sell a call and a put at different strikes, same expiration (volatility sale).\n\n\
            HARDCODED: direction=SELL_TO_OPEN for both legs.\n\
            Order type: NET_CREDIT.\n\n\
            WARNING: SIGNIFICANT RISK from large moves beyond either strike.\n\
            Max loss: unlimited on upside, (put strike - premium) on downside.\n\
            Max gain: total premium received.\n\
            Profit when: underlying stays between the two strikes through expiration.\n\n\
            Example:\n  \
            schwab-agent order build short-strangle AAPL --expiration 2025-06-20 \\\n    \
            --call-strike 210 --put-strike 190 --price 6.00"
    )]
    ShortStrangle(StrangleArgs),

    /// Short iron condor: sell a put spread + call spread for net credit (neutral).
    ///
    /// Hardcoded: all four directions locked. Net credit received.
    /// Profits when the underlying stays between the two short strikes.
    #[command(
        name = "short-iron-condor",
        long_about = "Short iron condor: sell a put credit spread and a call credit spread (neutral).\n\n\
            HARDCODED directions (all opening):\n\
            - BUY_TO_OPEN put at put-long-strike (lowest, protective)\n\
            - SELL_TO_OPEN put at put-short-strike\n\
            - SELL_TO_OPEN call at call-short-strike\n\
            - BUY_TO_OPEN call at call-long-strike (highest, protective)\n\
            Order type: NET_CREDIT.\n\n\
            Strikes must be ordered: put-long < put-short < call-short < call-long.\n\n\
            Max loss: width of wider spread minus credit received.\n\
            Max gain: net credit received.\n\
            Profit when: underlying stays between put-short and call-short strikes.\n\n\
            Example:\n  \
            schwab-agent order build short-iron-condor SPY --expiration 2025-06-20 \\\n    \
            --put-long-strike 400 --put-short-strike 410 \\\n    \
            --call-short-strike 440 --call-long-strike 450 --price 3.00"
    )]
    ShortIronCondor(IronCondorArgs),

    /// Jade lizard: sell a put + sell a call + buy a higher call (neutral-bullish).
    ///
    /// Hardcoded: all three directions locked. Net credit received.
    /// The bought call provides upside protection, creating no risk to
    /// the upside if the credit received exceeds the call spread width.
    #[command(
        name = "jade-lizard",
        long_about = "Jade lizard: short put + short call spread for net credit (neutral to bullish).\n\n\
            HARDCODED directions (all opening):\n\
            - SELL_TO_OPEN put at put-strike\n\
            - SELL_TO_OPEN call at short-call-strike\n\
            - BUY_TO_OPEN call at long-call-strike (protective)\n\
            Order type: NET_CREDIT.\n\n\
            short-call-strike must be less than long-call-strike.\n\
            If the net credit received exceeds (long-call-strike - short-call-strike),\n\
            there is no risk to the upside.\n\n\
            Max loss on downside: (put-strike - credit received) x 100.\n\
            Max loss on upside: (long-call - short-call - credit) x 100 (can be zero or negative).\n\
            Max gain: net credit received.\n\n\
            Example:\n  \
            schwab-agent order build jade-lizard AAPL --expiration 2025-06-20 \\\n    \
            --put-strike 180 --short-call-strike 210 --long-call-strike 220 --price 5.00"
    )]
    JadeLizard(JadeLizardArgs),
}

// ---------------------------------------------------------------------------
// Order command enum
// ---------------------------------------------------------------------------

/// Order construction, preview, placement, replacement, and lifecycle management.
#[derive(Debug, Subcommand)]
pub enum OrderCommand {
    /// Construct the order JSON locally without calling the API.
    ///
    /// Useful for inspecting the exact payload before previewing or placing.
    #[command(subcommand)]
    Build(StrategyCommand),

    /// Preview the order via the Schwab API (no funds committed).
    ///
    /// Returns estimated commissions, fees, and validation results.
    /// Add --save-preview to persist the payload as a tamper-evident digest
    /// that can later be placed with `order place-from-preview`.
    Preview(OrderPreviewArgs),

    /// Place the order directly via the Schwab API.
    ///
    /// Builds the order from the strategy arguments and submits it.
    /// The order will be live immediately upon acceptance. Includes
    /// post-place verification via a follow-up GET.
    Place(OrderPlaceArgs),

    /// Place an order from a previously saved preview digest.
    ///
    /// Loads the exact order payload that was previewed, verifies the
    /// digest for tamper detection, checks the 15-minute TTL, and submits
    /// the order to the Schwab API. Includes post-place verification.
    #[command(name = "place-from-preview")]
    PlaceFromPreview(PlaceFromPreviewArgs),

    /// Replace an existing order with a newly built strategy payload.
    ///
    /// Builds the replacement payload from the same safe strategy commands used
    /// by build/preview/place, submits it via Schwab's replace endpoint, and
    /// verifies the resulting order with a follow-up GET.
    Replace(OrderReplaceArgs),

    /// List orders for an account or all linked accounts.
    ///
    /// Returns orders within a time range (default: last 60 days).
    /// Use --recent for the last 24 hours. Optionally filter by status.
    List(lifecycle::OrderListArgs),

    /// Retrieve a single order by ID.
    Get(lifecycle::OrderGetArgs),

    /// Cancel an order by ID.
    ///
    /// After cancellation, verifies the status via a follow-up GET so
    /// the agent can confirm the order was actually canceled.
    Cancel(lifecycle::OrderCancelArgs),
}

/// Arguments for `order preview <strategy>`.
#[derive(Debug, Args)]
pub struct OrderPreviewArgs {
    /// Account hash or nickname (use `account summary` to list accounts).
    #[arg(long)]
    pub account: String,

    /// Save the preview payload as a tamper-evident digest for later placement.
    #[arg(long)]
    pub save_preview: bool,

    /// The strategy to preview.
    #[command(subcommand)]
    pub strategy: StrategyCommand,
}

/// Arguments for `order place <strategy>`.
#[derive(Debug, Args)]
pub struct OrderPlaceArgs {
    /// Account hash or nickname (use `account summary` to list accounts).
    #[arg(long)]
    pub account: String,

    /// The strategy to place.
    #[command(subcommand)]
    pub strategy: StrategyCommand,
}

/// Arguments for `order place-from-preview`.
#[derive(Debug, Args)]
pub struct PlaceFromPreviewArgs {
    /// Account hash or nickname (must resolve to the hash used during preview).
    #[arg(long)]
    pub account: String,

    /// SHA-256 digest from a previous `order preview --save-preview` run.
    #[arg(long)]
    pub digest: String,
}

/// Arguments for `order replace <order-id> <strategy>`.
#[derive(Debug, Args)]
pub struct OrderReplaceArgs {
    /// Account hash or nickname (use `account summary` to list accounts).
    #[arg(long)]
    pub account: String,

    /// Existing order ID to replace.
    #[arg(value_parser = clap::value_parser!(i64).range(1..))]
    pub order_id: i64,

    /// Replacement strategy payload.
    #[command(subcommand)]
    pub strategy: StrategyCommand,
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

/// Handles all order subcommands and returns a complete JSON envelope.
///
/// Order commands bypass the generic envelope wrapping in `execute()` because
/// they compute their own dynamic command names (e.g., `order.build.long-call`).
pub(crate) async fn handle(cli: &Cli, command: &OrderCommand) -> Result<CommandOutput, AppError> {
    match command {
        OrderCommand::Build(strategy) => {
            let name = format!("order.build.{}", strategy_name(strategy));
            let data = do_build(strategy)?;
            Ok(Envelope::success(&name, data, Metadata::now()))
        }
        OrderCommand::Preview(args) => {
            let name = format!("order.preview.{}", strategy_name(&args.strategy));
            let data =
                do_preview(cli, &args.account, &args.strategy, args.save_preview, &name).await?;
            Ok(Envelope::success(&name, data, Metadata::now()))
        }
        OrderCommand::Place(args) => {
            let name = format!("order.place.{}", strategy_name(&args.strategy));
            do_place(cli, &args.account, &args.strategy, &name).await
        }
        OrderCommand::PlaceFromPreview(args) => {
            do_place_from_preview(cli, &args.account, &args.digest).await
        }
        OrderCommand::Replace(args) => {
            do_replace(cli, &args.account, args.order_id, &args.strategy).await
        }
        OrderCommand::List(args) => lifecycle::handle_list(cli, args).await,
        OrderCommand::Get(args) => lifecycle::handle_get(cli, args).await,
        OrderCommand::Cancel(args) => lifecycle::handle_cancel(cli, args).await,
    }
}

/// Returns the kebab-case strategy name for use in command names.
#[must_use]
fn strategy_name(strategy: &StrategyCommand) -> &'static str {
    match strategy {
        StrategyCommand::LongCall(_) => "long-call",
        StrategyCommand::LongPut(_) => "long-put",
        StrategyCommand::CashSecuredPut(_) => "cash-secured-put",
        StrategyCommand::NakedCall(_) => "naked-call",
        StrategyCommand::SellCoveredCall(_) => "sell-covered-call",
        StrategyCommand::PutCreditSpread(_) => "put-credit-spread",
        StrategyCommand::CallCreditSpread(_) => "call-credit-spread",
        StrategyCommand::PutDebitSpread(_) => "put-debit-spread",
        StrategyCommand::CallDebitSpread(_) => "call-debit-spread",
        StrategyCommand::LongStraddle(_) => "long-straddle",
        StrategyCommand::ShortStraddle(_) => "short-straddle",
        StrategyCommand::LongStrangle(_) => "long-strangle",
        StrategyCommand::ShortStrangle(_) => "short-strangle",
        StrategyCommand::ShortIronCondor(_) => "short-iron-condor",
        StrategyCommand::JadeLizard(_) => "jade-lizard",
    }
}

// ---------------------------------------------------------------------------
// Order construction (strategy -> OptionOrder)
// ---------------------------------------------------------------------------

/// Builds an `OptionOrder` payload from a strategy command.
///
/// Each strategy hardcodes the contract type and instruction so that
/// callers cannot accidentally reverse a trade.
fn build_order(strategy: &StrategyCommand) -> Result<OptionOrder, AppError> {
    use schwab::{Instruction, PutCall};

    match strategy {
        // -- Single-leg strategies --
        StrategyCommand::LongCall(a) => builder::build_single_leg(
            &a.underlying,
            &a.expiration,
            a.strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Call,
            Instruction::BuyToOpen,
        ),
        StrategyCommand::LongPut(a) => builder::build_single_leg(
            &a.underlying,
            &a.expiration,
            a.strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Put,
            Instruction::BuyToOpen,
        ),
        StrategyCommand::CashSecuredPut(a) => builder::build_single_leg(
            &a.underlying,
            &a.expiration,
            a.strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Put,
            Instruction::SellToOpen,
        ),
        StrategyCommand::NakedCall(a) => builder::build_single_leg(
            &a.underlying,
            &a.expiration,
            a.strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Call,
            Instruction::SellToOpen,
        ),
        StrategyCommand::SellCoveredCall(a) => builder::build_single_leg(
            &a.underlying,
            &a.expiration,
            a.strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Call,
            Instruction::SellToOpen,
        ),

        // -- Vertical spreads --
        // Put credit spread (bull put): sell high put, buy low put
        StrategyCommand::PutCreditSpread(a) => builder::build_vertical(
            &a.underlying,
            &a.expiration,
            a.high_strike,
            a.low_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Put,
            false, // long_is_high=false: buy the LOW strike put
            true,  // is_credit=true
        ),
        // Call credit spread (bear call): sell low call, buy high call
        StrategyCommand::CallCreditSpread(a) => builder::build_vertical(
            &a.underlying,
            &a.expiration,
            a.high_strike,
            a.low_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Call,
            true, // long_is_high=true: buy the HIGH strike call
            true, // is_credit=true
        ),
        // Put debit spread (bear put): buy high put, sell low put
        StrategyCommand::PutDebitSpread(a) => builder::build_vertical(
            &a.underlying,
            &a.expiration,
            a.high_strike,
            a.low_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Put,
            true,  // long_is_high=true: buy the HIGH strike put
            false, // is_credit=false
        ),
        // Call debit spread (bull call): buy low call, sell high call
        StrategyCommand::CallDebitSpread(a) => builder::build_vertical(
            &a.underlying,
            &a.expiration,
            a.high_strike,
            a.low_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            PutCall::Call,
            false, // long_is_high=false: buy the LOW strike call
            false, // is_credit=false
        ),

        // -- Straddles --
        StrategyCommand::LongStraddle(a) => builder::build_straddle(
            &a.underlying,
            &a.expiration,
            a.strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            true, // is_buy=true
        ),
        StrategyCommand::ShortStraddle(a) => builder::build_straddle(
            &a.underlying,
            &a.expiration,
            a.strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            false, // is_buy=false
        ),

        // -- Strangles --
        StrategyCommand::LongStrangle(a) => builder::build_strangle(
            &a.underlying,
            &a.expiration,
            a.call_strike,
            a.put_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            true, // is_buy=true
        ),
        StrategyCommand::ShortStrangle(a) => builder::build_strangle(
            &a.underlying,
            &a.expiration,
            a.call_strike,
            a.put_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
            false, // is_buy=false
        ),

        // -- Iron condor --
        StrategyCommand::ShortIronCondor(a) => builder::build_iron_condor(
            &a.underlying,
            &a.expiration,
            a.put_long_strike,
            a.put_short_strike,
            a.call_short_strike,
            a.call_long_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
        ),

        // -- Jade lizard --
        StrategyCommand::JadeLizard(a) => builder::build_jade_lizard(
            &a.underlying,
            &a.expiration,
            a.put_strike,
            a.short_call_strike,
            a.long_call_strike,
            a.quantity,
            a.price,
            a.session.into(),
            a.duration.into(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Build / Preview / Place handlers
// ---------------------------------------------------------------------------

/// Serializes an `OptionOrder` to a JSON `Value`.
fn serialize_order(order: &OptionOrder) -> Result<Value, AppError> {
    serde_json::to_value(order)
        .map_err(|e| AppError::OrderValidation(format!("failed to serialize order: {e}")))
}

/// Builds the order JSON locally without any API call.
fn do_build(strategy: &StrategyCommand) -> Result<Value, AppError> {
    let order = build_order(strategy)?;
    serialize_order(&order)
}

/// Previews the order via the Schwab API.
async fn do_preview(
    cli: &Cli,
    account: &str,
    strategy: &StrategyCommand,
    save: bool,
    command_name: &str,
) -> Result<Value, AppError> {
    let order = build_order(strategy)?;
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
async fn do_place(
    cli: &Cli,
    account: &str,
    strategy: &StrategyCommand,
    command_name: &str,
) -> Result<CommandOutput, AppError> {
    let order = build_order(strategy)?;
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

    verify::action_envelope(command_name, result)
}

/// Places an order from a previously saved preview digest with post-place
/// verification.
async fn do_place_from_preview(
    cli: &Cli,
    account: &str,
    digest: &str,
) -> Result<CommandOutput, AppError> {
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

    verify::action_envelope("order.place-from-preview", result)
}

/// Replaces an existing order with a strategy payload and verifies the result.
async fn do_replace(
    cli: &Cli,
    account: &str,
    order_id: i64,
    strategy: &StrategyCommand,
) -> Result<CommandOutput, AppError> {
    let order = build_order(strategy)?;
    let client = auth::provider(cli)?.client().await?;
    let resolved = account::resolve_account(&client, account).await?;
    let account_hash = resolved.account_hash;
    let response = client
        .replace_order(&account_hash, order_id, &order)
        .await?;
    let order_json = serialize_order(&order)?;
    let new_order_id = response.order_id.ok_or(AppError::OrderValidation(
        "replace response did not include the new order ID required for verification".to_string(),
    ))?;

    let result = verify::verify_order(
        &client,
        &account_hash,
        Some(new_order_id),
        "replace",
        response.location,
        Some(order_json),
    )
    .await;

    verify::action_envelope("order.replace", result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;

    #[test]
    fn resolve_by_nickname_returns_canonical_hash() {
        use crate::account::resolve_account_from_data;
        use schwab::{AccountNumberHash, UserPreferenceAccount};

        let hashes = vec![AccountNumberHash {
            account_number: Some("123456".to_string()),
            hash_value: Some("HASH123".to_string()),
        }];
        let prefs = vec![UserPreferenceAccount {
            account_color: None,
            account_number: Some("123456".to_string()),
            auto_position_effect: None,
            display_acct_id: None,
            nick_name: Some("Trading".to_string()),
            primary_account: None,
            r#type: None,
        }];

        let result = resolve_account_from_data(&hashes, &prefs, "Trading");
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert_eq!(resolved.account_hash, "HASH123");
        assert_eq!(resolved.matched_by, "nickname");
    }

    #[test]
    fn parse_build_long_call() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "build",
            "long-call",
            "AAPL",
            "--expiration",
            "2025-06-20",
            "--strike",
            "200",
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn parse_build_long_call_with_price() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "build",
            "long-call",
            "AAPL",
            "--expiration",
            "2025-06-20",
            "--strike",
            "200",
            "--price",
            "5.50",
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn parse_preview_put_credit_spread() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "preview",
            "--account",
            "ABC123",
            "put-credit-spread",
            "AAPL",
            "--expiration",
            "2025-06-20",
            "--high-strike",
            "200",
            "--low-strike",
            "190",
            "--price",
            "3.00",
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn parse_preview_with_save() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "preview",
            "--account",
            "ABC123",
            "--save-preview",
            "long-call",
            "AAPL",
            "--expiration",
            "2025-06-20",
            "--strike",
            "200",
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn parse_place_short_iron_condor() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "place",
            "--account",
            "ABC123",
            "short-iron-condor",
            "SPY",
            "--expiration",
            "2025-06-20",
            "--put-long-strike",
            "400",
            "--put-short-strike",
            "410",
            "--call-short-strike",
            "440",
            "--call-long-strike",
            "450",
            "--price",
            "3.00",
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn parse_place_from_preview() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "place-from-preview",
            "--account",
            "ABC123",
            "--digest",
            "a".repeat(64).as_str(),
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn parse_replace_long_call() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "replace",
            "--account",
            "ABC123",
            "12345678",
            "long-call",
            "AAPL",
            "--expiration",
            "2025-06-20",
            "--strike",
            "200",
            "--price",
            "5.50",
        ]);

        let crate::cli::Command::Order(crate::order::OrderCommand::Replace(args)) = cli.command
        else {
            panic!("expected order replace command");
        };
        assert_eq!(args.account, "ABC123");
        assert_eq!(args.order_id, 12_345_678);
        assert!(matches!(
            args.strategy,
            crate::order::StrategyCommand::LongCall(_)
        ));
    }

    #[test]
    fn parse_replace_rejects_non_positive_order_id() {
        assert!(
            Cli::try_parse_from([
                "schwab-agent",
                "order",
                "replace",
                "--account",
                "ABC123",
                "0",
                "long-call",
                "AAPL",
                "--expiration",
                "2025-06-20",
                "--strike",
                "200",
            ])
            .is_err()
        );
    }

    #[test]
    fn parse_jade_lizard() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "build",
            "jade-lizard",
            "AAPL",
            "--expiration",
            "2025-06-20",
            "--put-strike",
            "180",
            "--short-call-strike",
            "210",
            "--long-call-strike",
            "220",
            "--price",
            "5.00",
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn parse_session_and_duration() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "build",
            "long-call",
            "AAPL",
            "--expiration",
            "2025-06-20",
            "--strike",
            "200",
            "--session",
            "seamless",
            "--duration",
            "gtc",
        ]);
        assert!(matches!(cli.command, crate::cli::Command::Order(_)));
    }

    #[test]
    fn strategy_name_coverage() {
        use super::{SingleLegArgs, StrategyCommand, strategy_name};
        use crate::shared::{DurationChoice, SessionChoice};
        let args = SingleLegArgs {
            underlying: "X".to_string(),
            expiration: "2025-01-01".to_string(),
            strike: 100.0,
            quantity: 1,
            price: None,
            session: SessionChoice::Normal,
            duration: DurationChoice::Day,
        };
        assert_eq!(strategy_name(&StrategyCommand::LongCall(args)), "long-call");
    }

    #[test]
    fn build_order_long_call_hardcodes_direction() {
        use super::{SingleLegArgs, StrategyCommand, build_order};
        use crate::shared::{DurationChoice, SessionChoice};
        let args = SingleLegArgs {
            underlying: "AAPL".to_string(),
            expiration: "2025-06-20".to_string(),
            strike: 200.0,
            quantity: 1,
            price: Some(5.0),
            session: SessionChoice::Normal,
            duration: DurationChoice::Day,
        };
        let order = build_order(&StrategyCommand::LongCall(args)).unwrap();
        assert_eq!(
            order.order_leg_collection[0].instruction,
            schwab::Instruction::BuyToOpen
        );
        assert_eq!(
            order.order_leg_collection[0].instrument.put_call,
            schwab::PutCall::Call
        );
    }

    #[test]
    fn build_order_cash_secured_put_hardcodes_direction() {
        use super::{SingleLegArgs, StrategyCommand, build_order};
        use crate::shared::{DurationChoice, SessionChoice};
        let args = SingleLegArgs {
            underlying: "AAPL".to_string(),
            expiration: "2025-06-20".to_string(),
            strike: 170.0,
            quantity: 1,
            price: Some(2.50),
            session: SessionChoice::Normal,
            duration: DurationChoice::Day,
        };
        let order = build_order(&StrategyCommand::CashSecuredPut(args)).unwrap();
        assert_eq!(
            order.order_leg_collection[0].instruction,
            schwab::Instruction::SellToOpen
        );
        assert_eq!(
            order.order_leg_collection[0].instrument.put_call,
            schwab::PutCall::Put
        );
    }

    #[test]
    fn build_order_put_credit_spread_directions() {
        use super::{StrategyCommand, VerticalArgs, build_order};
        use crate::shared::{DurationChoice, SessionChoice};
        let args = VerticalArgs {
            underlying: "AAPL".to_string(),
            expiration: "2025-06-20".to_string(),
            high_strike: 200.0,
            low_strike: 190.0,
            quantity: 1,
            price: Some(3.0),
            session: SessionChoice::Normal,
            duration: DurationChoice::Day,
        };
        let order = build_order(&StrategyCommand::PutCreditSpread(args)).unwrap();
        // Should have 2 legs: buy low put, sell high put.
        assert_eq!(order.order_leg_collection.len(), 2);

        let legs = &order.order_leg_collection;
        // Long leg (buy) is the low strike.
        let buy_leg = legs
            .iter()
            .find(|l| l.instruction == schwab::Instruction::BuyToOpen)
            .unwrap();
        assert_eq!(buy_leg.instrument.option_strike_price, Some(190.0));
        // Short leg (sell) is the high strike.
        let sell_leg = legs
            .iter()
            .find(|l| l.instruction == schwab::Instruction::SellToOpen)
            .unwrap();
        assert_eq!(sell_leg.instrument.option_strike_price, Some(200.0));
    }

    #[test]
    fn do_build_returns_valid_json() {
        use super::{SingleLegArgs, StrategyCommand, do_build};
        use crate::shared::{DurationChoice, SessionChoice};
        let args = SingleLegArgs {
            underlying: "AAPL".to_string(),
            expiration: "2025-06-20".to_string(),
            strike: 200.0,
            quantity: 1,
            price: Some(5.0),
            session: SessionChoice::Normal,
            duration: DurationChoice::Day,
        };
        let value = do_build(&StrategyCommand::LongCall(args)).unwrap();
        assert!(value.is_object());
        assert!(value.get("orderType").is_some());
        assert!(value.get("orderLegCollection").is_some());
    }
}
