//! Option order payload construction.
//!
//! Builds Schwab API order payloads for options strategies. The schwab-rs
//! `OrderBuilder` only supports equities, so this module constructs option
//! order payloads directly using schwab-rs enum types for correct serialization.

use schwab::{
    ComplexOrderStrategyType, Duration, Instruction, InstrumentAssetType, OrderStrategyType,
    OrderTypeRequest, PutCall, Session,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Order payload types (serialized to Schwab API JSON)
// ---------------------------------------------------------------------------

/// Complete option order payload sent to the Schwab API.
///
/// Serializes to `camelCase` JSON matching the Schwab order schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionOrder {
    /// Trading session for the order.
    pub session: Session,
    /// Time-in-force for the order.
    pub duration: Duration,
    /// Order price type (Market, Limit, NetDebit, NetCredit, etc.).
    pub order_type: OrderTypeRequest,
    /// Multi-leg strategy classification.
    pub complex_order_strategy_type: ComplexOrderStrategyType,
    /// Always `Single` for standalone orders.
    pub order_strategy_type: OrderStrategyType,
    /// Limit price. Omitted for market orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// One or more option legs.
    pub order_leg_collection: Vec<OptionLeg>,
}

/// A single leg of an option order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionLeg {
    /// Direction: BUY_TO_OPEN, SELL_TO_OPEN, BUY_TO_CLOSE, SELL_TO_CLOSE.
    pub instruction: Instruction,
    /// Number of contracts.
    pub quantity: u32,
    /// Instrument details including OCC symbol.
    pub instrument: OptionInstrument,
}

/// Option instrument details within an order leg.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionInstrument {
    /// Standard OCC option symbol (21 characters).
    pub symbol: String,
    /// Always `OPTION` for option orders.
    pub asset_type: InstrumentAssetType,
    /// `CALL` or `PUT`.
    pub put_call: PutCall,
    /// Underlying ticker symbol (e.g., `"AAPL"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underlying_symbol: Option<String>,
    /// Contract multiplier, typically `100.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_multiplier: Option<f64>,
    /// Expiration date as `YYYY-MM-DD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_expiration_date: Option<String>,
    /// Strike price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_strike_price: Option<f64>,
}

// ---------------------------------------------------------------------------
// OCC symbol construction
// ---------------------------------------------------------------------------

/// Builds a standard OCC option symbol.
///
/// Format: `{underlying:6}{YYMMDD}{C|P}{strike*1000:08}` (21 characters).
/// The underlying is right-padded with spaces to 6 characters.
///
/// # Errors
///
/// Returns [`AppError::OrderValidation`] if the underlying exceeds 6 characters
/// or the expiration is not in `YYYY-MM-DD` format.
pub fn occ_symbol(
    underlying: &str,
    expiration: &str,
    strike: f64,
    put_call: &PutCall,
) -> Result<String, AppError> {
    if underlying.len() > 6 {
        return Err(AppError::OrderValidation(format!(
            "underlying symbol '{underlying}' exceeds 6 characters"
        )));
    }
    let padded = format!("{:<6}", underlying.to_uppercase());

    let yymmdd = expiration_to_yymmdd(expiration)?;

    let pc = match put_call {
        PutCall::Call => 'C',
        PutCall::Put => 'P',
        _ => {
            return Err(AppError::OrderValidation(
                "unknown put_call variant".to_string(),
            ));
        }
    };

    let strike_int = (strike * 1000.0) as u64;

    Ok(format!("{padded}{yymmdd}{pc}{strike_int:08}"))
}

/// Converts `YYYY-MM-DD` to `YYMMDD`.
fn expiration_to_yymmdd(expiration: &str) -> Result<String, AppError> {
    let bytes = expiration.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(AppError::OrderValidation(format!(
            "expiration must be YYYY-MM-DD, got: {expiration}"
        )));
    }
    Ok(format!(
        "{}{}{}",
        &expiration[2..4],
        &expiration[5..7],
        &expiration[8..10]
    ))
}

// ---------------------------------------------------------------------------
// Leg builder
// ---------------------------------------------------------------------------

/// Builds a single option leg with full instrument details.
pub fn option_leg(
    underlying: &str,
    expiration: &str,
    strike: f64,
    put_call: PutCall,
    instruction: Instruction,
    quantity: u32,
) -> Result<OptionLeg, AppError> {
    let symbol = occ_symbol(underlying, expiration, strike, &put_call)?;
    Ok(OptionLeg {
        instruction,
        quantity,
        instrument: OptionInstrument {
            symbol,
            asset_type: InstrumentAssetType::Option,
            put_call,
            underlying_symbol: Some(underlying.to_uppercase()),
            option_multiplier: Some(100.0),
            option_expiration_date: Some(expiration.to_string()),
            option_strike_price: Some(strike),
        },
    })
}

// ---------------------------------------------------------------------------
// Strategy builders
// ---------------------------------------------------------------------------

/// Builds a single-leg option order (long call, long put, cash-secured put,
/// naked call, or sell covered call).
pub fn build_single_leg(
    underlying: &str,
    expiration: &str,
    strike: f64,
    quantity: u32,
    price: Option<f64>,
    session: Session,
    duration: Duration,
    put_call: PutCall,
    instruction: Instruction,
) -> Result<OptionOrder, AppError> {
    let order_type = match price {
        Some(_) => OrderTypeRequest::Limit,
        None => OrderTypeRequest::Market,
    };
    let leg = option_leg(
        underlying,
        expiration,
        strike,
        put_call,
        instruction,
        quantity,
    )?;
    Ok(OptionOrder {
        session,
        duration,
        order_type,
        complex_order_strategy_type: ComplexOrderStrategyType::None,
        order_strategy_type: OrderStrategyType::Single,
        price,
        order_leg_collection: vec![leg],
    })
}

/// Builds a vertical spread order.
///
/// `long_is_high` controls which strike is bought vs sold:
/// - `true`:  buy high strike, sell low strike (bear put, bull call debit inverted? No...)
/// - `false`: buy low strike, sell high strike
///
/// The `is_credit` flag determines the order type when a price is given.
pub fn build_vertical(
    underlying: &str,
    expiration: &str,
    high_strike: f64,
    low_strike: f64,
    quantity: u32,
    price: Option<f64>,
    session: Session,
    duration: Duration,
    put_call: PutCall,
    long_is_high: bool,
    is_credit: bool,
) -> Result<OptionOrder, AppError> {
    if high_strike <= low_strike {
        return Err(AppError::OrderValidation(format!(
            "high-strike ({high_strike}) must be greater than low-strike ({low_strike})"
        )));
    }

    let (buy_instruction, sell_instruction) = (Instruction::BuyToOpen, Instruction::SellToOpen);

    let (long_strike, short_strike) = if long_is_high {
        (high_strike, low_strike)
    } else {
        (low_strike, high_strike)
    };

    let long_leg = option_leg(
        underlying,
        expiration,
        long_strike,
        put_call.clone(),
        buy_instruction,
        quantity,
    )?;
    let short_leg = option_leg(
        underlying,
        expiration,
        short_strike,
        put_call,
        sell_instruction,
        quantity,
    )?;

    let order_type = match price {
        Some(_) if is_credit => OrderTypeRequest::NetCredit,
        Some(_) => OrderTypeRequest::NetDebit,
        None => OrderTypeRequest::Market,
    };

    Ok(OptionOrder {
        session,
        duration,
        order_type,
        complex_order_strategy_type: ComplexOrderStrategyType::Vertical,
        order_strategy_type: OrderStrategyType::Single,
        price,
        order_leg_collection: vec![long_leg, short_leg],
    })
}

/// Builds a straddle order (long or short, same strike for call and put).
pub fn build_straddle(
    underlying: &str,
    expiration: &str,
    strike: f64,
    quantity: u32,
    price: Option<f64>,
    session: Session,
    duration: Duration,
    is_buy: bool,
) -> Result<OptionOrder, AppError> {
    let instruction = if is_buy {
        Instruction::BuyToOpen
    } else {
        Instruction::SellToOpen
    };

    let call_leg = option_leg(
        underlying,
        expiration,
        strike,
        PutCall::Call,
        instruction.clone(),
        quantity,
    )?;
    let put_leg = option_leg(
        underlying,
        expiration,
        strike,
        PutCall::Put,
        instruction,
        quantity,
    )?;

    let order_type = match price {
        Some(_) if is_buy => OrderTypeRequest::NetDebit,
        Some(_) => OrderTypeRequest::NetCredit,
        None => OrderTypeRequest::Market,
    };

    Ok(OptionOrder {
        session,
        duration,
        order_type,
        complex_order_strategy_type: ComplexOrderStrategyType::Straddle,
        order_strategy_type: OrderStrategyType::Single,
        price,
        order_leg_collection: vec![call_leg, put_leg],
    })
}

/// Builds a strangle order (long or short, different strikes for call and put).
pub fn build_strangle(
    underlying: &str,
    expiration: &str,
    call_strike: f64,
    put_strike: f64,
    quantity: u32,
    price: Option<f64>,
    session: Session,
    duration: Duration,
    is_buy: bool,
) -> Result<OptionOrder, AppError> {
    let instruction = if is_buy {
        Instruction::BuyToOpen
    } else {
        Instruction::SellToOpen
    };

    let call_leg = option_leg(
        underlying,
        expiration,
        call_strike,
        PutCall::Call,
        instruction.clone(),
        quantity,
    )?;
    let put_leg = option_leg(
        underlying,
        expiration,
        put_strike,
        PutCall::Put,
        instruction,
        quantity,
    )?;

    let order_type = match price {
        Some(_) if is_buy => OrderTypeRequest::NetDebit,
        Some(_) => OrderTypeRequest::NetCredit,
        None => OrderTypeRequest::Market,
    };

    Ok(OptionOrder {
        session,
        duration,
        order_type,
        complex_order_strategy_type: ComplexOrderStrategyType::Strangle,
        order_strategy_type: OrderStrategyType::Single,
        price,
        order_leg_collection: vec![call_leg, put_leg],
    })
}

/// Builds a short iron condor order (four legs, net credit).
///
/// Legs (all opening):
/// - BUY put at `put_long_strike` (lowest, protective)
/// - SELL put at `put_short_strike`
/// - SELL call at `call_short_strike`
/// - BUY call at `call_long_strike` (highest, protective)
pub fn build_iron_condor(
    underlying: &str,
    expiration: &str,
    put_long_strike: f64,
    put_short_strike: f64,
    call_short_strike: f64,
    call_long_strike: f64,
    quantity: u32,
    price: Option<f64>,
    session: Session,
    duration: Duration,
) -> Result<OptionOrder, AppError> {
    // Validate strike ordering: put_long < put_short < call_short < call_long
    if put_long_strike >= put_short_strike {
        return Err(AppError::OrderValidation(format!(
            "put-long-strike ({put_long_strike}) must be less than put-short-strike ({put_short_strike})"
        )));
    }
    if put_short_strike >= call_short_strike {
        return Err(AppError::OrderValidation(format!(
            "put-short-strike ({put_short_strike}) must be less than call-short-strike ({call_short_strike})"
        )));
    }
    if call_short_strike >= call_long_strike {
        return Err(AppError::OrderValidation(format!(
            "call-short-strike ({call_short_strike}) must be less than call-long-strike ({call_long_strike})"
        )));
    }

    let legs = vec![
        option_leg(
            underlying,
            expiration,
            put_long_strike,
            PutCall::Put,
            Instruction::BuyToOpen,
            quantity,
        )?,
        option_leg(
            underlying,
            expiration,
            put_short_strike,
            PutCall::Put,
            Instruction::SellToOpen,
            quantity,
        )?,
        option_leg(
            underlying,
            expiration,
            call_short_strike,
            PutCall::Call,
            Instruction::SellToOpen,
            quantity,
        )?,
        option_leg(
            underlying,
            expiration,
            call_long_strike,
            PutCall::Call,
            Instruction::BuyToOpen,
            quantity,
        )?,
    ];

    let order_type = match price {
        Some(_) => OrderTypeRequest::NetCredit,
        None => OrderTypeRequest::Market,
    };

    Ok(OptionOrder {
        session,
        duration,
        order_type,
        complex_order_strategy_type: ComplexOrderStrategyType::IronCondor,
        order_strategy_type: OrderStrategyType::Single,
        price,
        order_leg_collection: legs,
    })
}

/// Builds a jade lizard order (three legs, net credit).
///
/// Legs (all opening):
/// - SELL put at `put_strike`
/// - SELL call at `short_call_strike`
/// - BUY call at `long_call_strike` (protective)
pub fn build_jade_lizard(
    underlying: &str,
    expiration: &str,
    put_strike: f64,
    short_call_strike: f64,
    long_call_strike: f64,
    quantity: u32,
    price: Option<f64>,
    session: Session,
    duration: Duration,
) -> Result<OptionOrder, AppError> {
    if short_call_strike >= long_call_strike {
        return Err(AppError::OrderValidation(format!(
            "short-call-strike ({short_call_strike}) must be less than long-call-strike ({long_call_strike})"
        )));
    }

    let legs = vec![
        option_leg(
            underlying,
            expiration,
            put_strike,
            PutCall::Put,
            Instruction::SellToOpen,
            quantity,
        )?,
        option_leg(
            underlying,
            expiration,
            short_call_strike,
            PutCall::Call,
            Instruction::SellToOpen,
            quantity,
        )?,
        option_leg(
            underlying,
            expiration,
            long_call_strike,
            PutCall::Call,
            Instruction::BuyToOpen,
            quantity,
        )?,
    ];

    let order_type = match price {
        Some(_) => OrderTypeRequest::NetCredit,
        None => OrderTypeRequest::Market,
    };

    Ok(OptionOrder {
        session,
        duration,
        order_type,
        complex_order_strategy_type: ComplexOrderStrategyType::Custom,
        order_strategy_type: OrderStrategyType::Single,
        price,
        order_leg_collection: legs,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occ_symbol_aapl_call() {
        let symbol = occ_symbol("AAPL", "2025-01-17", 200.0, &PutCall::Call).unwrap();
        assert_eq!(symbol, "AAPL  250117C00200000");
    }

    #[test]
    fn occ_symbol_aapl_put() {
        let symbol = occ_symbol("AAPL", "2025-01-17", 150.5, &PutCall::Put).unwrap();
        assert_eq!(symbol, "AAPL  250117P00150500");
    }

    #[test]
    fn occ_symbol_short_ticker() {
        let symbol = occ_symbol("F", "2026-06-19", 12.0, &PutCall::Call).unwrap();
        assert_eq!(symbol, "F     260619C00012000");
    }

    #[test]
    fn occ_symbol_six_char_ticker() {
        let symbol = occ_symbol("GOOGLL", "2025-03-21", 175.0, &PutCall::Put).unwrap();
        assert_eq!(symbol, "GOOGLL250321P00175000");
    }

    #[test]
    fn occ_symbol_rejects_long_ticker() {
        let result = occ_symbol("TOOLONG", "2025-01-17", 100.0, &PutCall::Call);
        assert!(result.is_err());
    }

    #[test]
    fn occ_symbol_rejects_bad_date() {
        let result = occ_symbol("AAPL", "20250117", 100.0, &PutCall::Call);
        assert!(result.is_err());
    }

    #[test]
    fn build_single_leg_market_order() {
        let order = build_single_leg(
            "AAPL",
            "2025-01-17",
            200.0,
            1,
            None,
            Session::Normal,
            Duration::Day,
            PutCall::Call,
            Instruction::BuyToOpen,
        )
        .unwrap();

        assert_eq!(order.order_type, OrderTypeRequest::Market);
        assert!(order.price.is_none());
        assert_eq!(order.order_leg_collection.len(), 1);
        assert_eq!(
            order.order_leg_collection[0].instruction,
            Instruction::BuyToOpen
        );
        assert_eq!(
            order.order_leg_collection[0].instrument.put_call,
            PutCall::Call
        );
    }

    #[test]
    fn build_single_leg_limit_order() {
        let order = build_single_leg(
            "AAPL",
            "2025-01-17",
            200.0,
            1,
            Some(5.50),
            Session::Normal,
            Duration::Day,
            PutCall::Put,
            Instruction::SellToOpen,
        )
        .unwrap();

        assert_eq!(order.order_type, OrderTypeRequest::Limit);
        assert_eq!(order.price, Some(5.50));
    }

    #[test]
    fn build_vertical_validates_strikes() {
        let result = build_vertical(
            "AAPL",
            "2025-01-17",
            100.0,
            200.0, // low > high, backwards
            1,
            Some(1.0),
            Session::Normal,
            Duration::Day,
            PutCall::Put,
            false,
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn build_vertical_credit_spread() {
        let order = build_vertical(
            "AAPL",
            "2025-01-17",
            210.0,
            200.0,
            1,
            Some(2.50),
            Session::Normal,
            Duration::Day,
            PutCall::Put,
            false, // long is low strike
            true,  // credit spread
        )
        .unwrap();

        assert_eq!(order.order_type, OrderTypeRequest::NetCredit);
        assert_eq!(
            order.complex_order_strategy_type,
            ComplexOrderStrategyType::Vertical
        );
        assert_eq!(order.order_leg_collection.len(), 2);
    }

    #[test]
    fn build_straddle_long() {
        let order = build_straddle(
            "SPY",
            "2025-03-21",
            450.0,
            1,
            Some(10.0),
            Session::Normal,
            Duration::Day,
            true, // long
        )
        .unwrap();

        assert_eq!(order.order_type, OrderTypeRequest::NetDebit);
        assert_eq!(
            order.complex_order_strategy_type,
            ComplexOrderStrategyType::Straddle
        );
        assert_eq!(order.order_leg_collection.len(), 2);
        // Both legs should be BuyToOpen.
        for leg in &order.order_leg_collection {
            assert_eq!(leg.instruction, Instruction::BuyToOpen);
        }
    }

    #[test]
    fn build_iron_condor_validates_strike_ordering() {
        // put_short >= call_short (overlap)
        let result = build_iron_condor(
            "SPY",
            "2025-03-21",
            400.0,
            420.0,
            410.0,
            440.0,
            1,
            Some(2.0),
            Session::Normal,
            Duration::Day,
        );
        assert!(result.is_err());
    }

    #[test]
    fn build_iron_condor_valid() {
        let order = build_iron_condor(
            "SPY",
            "2025-03-21",
            400.0,
            410.0,
            430.0,
            440.0,
            1,
            Some(2.0),
            Session::Normal,
            Duration::Day,
        )
        .unwrap();

        assert_eq!(order.order_type, OrderTypeRequest::NetCredit);
        assert_eq!(
            order.complex_order_strategy_type,
            ComplexOrderStrategyType::IronCondor
        );
        assert_eq!(order.order_leg_collection.len(), 4);
    }

    #[test]
    fn build_jade_lizard_valid() {
        let order = build_jade_lizard(
            "AAPL",
            "2025-03-21",
            180.0,
            200.0,
            210.0,
            1,
            Some(3.0),
            Session::Normal,
            Duration::Day,
        )
        .unwrap();

        assert_eq!(order.order_type, OrderTypeRequest::NetCredit);
        assert_eq!(
            order.complex_order_strategy_type,
            ComplexOrderStrategyType::Custom
        );
        assert_eq!(order.order_leg_collection.len(), 3);
    }

    #[test]
    fn option_order_serializes_to_camel_case() {
        let order = build_single_leg(
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

        let json = serde_json::to_value(&order).unwrap();
        // Verify camelCase field names.
        assert!(json.get("orderType").is_some());
        assert!(json.get("complexOrderStrategyType").is_some());
        assert!(json.get("orderLegCollection").is_some());
        // Verify enum serialization (SCREAMING_SNAKE_CASE).
        assert_eq!(json["session"], "NORMAL");
        assert_eq!(json["duration"], "DAY");
        assert_eq!(json["orderType"], "LIMIT");
    }
}
