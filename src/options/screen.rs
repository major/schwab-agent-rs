use std::cmp::Ordering;

use schwab::{Client, Number, OptionChain, OptionChainOptions};
use serde_json::{Value, json, to_value};

use crate::cli::ScreenArgs;
use crate::error::AppError;
use crate::shared::to_number;

use super::types::{
    ALL_FIELDS, FlatContract, SCREEN_FIELDS, filter_by_ask, filter_by_bid, filter_by_delta,
    filter_by_oi, filter_by_premium, filter_by_spread_pct, filter_by_strike, filter_by_volume,
    flatten_chain, select_fields, sort_contracts, validate_fields,
};

/// Fetches an option chain and returns filtered `option screen` rows.
pub async fn handle(client: &Client, args: &ScreenArgs) -> Result<Value, AppError> {
    let options = build_chain_options(args);
    let chain = client.get_option_chain(options).await?;

    screen_chain(&chain, args)
}

pub(super) fn screen_chain(chain: &OptionChain, args: &ScreenArgs) -> Result<Value, AppError> {
    let underlying_price = underlying_price(chain);
    let mut contracts = flatten_chain(chain);
    sort_contracts(&mut contracts);

    if contracts.is_empty() {
        return Err(AppError::OptionsSymbolNotFound {
            symbol: args.symbol.clone(),
        });
    }

    let total_scanned = contracts.len();
    let mut filters_applied = Vec::new();

    apply_filters(&mut contracts, args, &mut filters_applied)?;
    apply_sort(&mut contracts, args, &mut filters_applied)?;

    if let Some(limit) = args.limit {
        contracts.truncate(limit);
    }

    let fields = selected_fields(args)?;
    let field_refs = fields.iter().map(String::as_str).collect::<Vec<_>>();
    let (columns, rows) = select_fields(&contracts, &field_refs);

    Ok(json!({
        "underlying": args.symbol,
        "underlyingPrice": underlying_price,
        "columns": columns,
        "rows": rows,
        "rowCount": rows.len(),
        "totalScanned": total_scanned,
        "filtersApplied": filters_applied,
    }))
}

fn build_chain_options(args: &ScreenArgs) -> OptionChainOptions {
    let mut options = OptionChainOptions::new(args.symbol.as_str())
        .parameter("strategy", "SINGLE")
        .include_underlying_quote(true);

    if let Some(contract_type) = args.contract_type.as_deref() {
        options = options.parameter("contractType", contract_type.to_uppercase());
    }
    if let Some(strike_range) = args.strike_range.as_deref() {
        options = options.parameter("range", strike_range);
    }

    options
}

fn apply_filters(
    contracts: &mut Vec<FlatContract>,
    args: &ScreenArgs,
    filters_applied: &mut Vec<String>,
) -> Result<(), AppError> {
    if let Some(contract_type) = normalized_contract_type(args.contract_type.as_deref())
        && contract_type != "ALL"
    {
        filters_applied.push(format!("type = {contract_type}"));
    }

    if let Some(dte_min) = args.dte_min {
        contracts.retain(|contract| contract.dte >= dte_min);
        filters_applied.push(format!("dte >= {dte_min}"));
    }
    if let Some(dte_max) = args.dte_max {
        contracts.retain(|contract| contract.dte <= dte_max);
        filters_applied.push(format!("dte <= {dte_max}"));
    }

    if args.strike_min.is_some() || args.strike_max.is_some() {
        let min = optional_number(args.strike_min)?;
        let max = optional_number(args.strike_max)?;
        contracts.retain(|contract| filter_by_strike(contract, min, max, None));
        if let Some(strike_min) = args.strike_min {
            filters_applied.push(format!("strike >= {}", format_number(strike_min)));
        }
        if let Some(strike_max) = args.strike_max {
            filters_applied.push(format!("strike <= {}", format_number(strike_max)));
        }
    }

    if let Some(strike) = args.strike {
        let exact = Some(number_arg(strike)?);
        contracts.retain(|contract| filter_by_strike(contract, None, None, exact));
        filters_applied.push(format!("strike = {}", format_number(strike)));
    }

    if args.delta_min.is_some() || args.delta_max.is_some() {
        let min = optional_number(args.delta_min)?;
        let max = optional_number(args.delta_max)?;
        contracts.retain(|contract| filter_by_delta(contract, min, max));
        if let Some(delta_min) = args.delta_min {
            filters_applied.push(format!("delta >= {}", format_number(delta_min)));
        }
        if let Some(delta_max) = args.delta_max {
            filters_applied.push(format!("delta <= {}", format_number(delta_max)));
        }
    }

    if let Some(min_bid) = args.min_bid {
        let min_bid_number = number_arg(min_bid)?;
        contracts.retain(|contract| filter_by_bid(contract, min_bid_number));
        filters_applied.push(format!("bid >= {}", format_number(min_bid)));
    }
    if let Some(max_ask) = args.max_ask {
        let max_ask_number = number_arg(max_ask)?;
        contracts.retain(|contract| filter_by_ask(contract, max_ask_number));
        filters_applied.push(format!("ask <= {}", format_number(max_ask)));
    }
    if let Some(min_volume) = args.min_volume {
        let min_volume_number = number_arg(min_volume as f64)?;
        contracts.retain(|contract| filter_by_volume(contract, min_volume_number));
        filters_applied.push(format!("volume >= {min_volume}"));
    }
    if let Some(min_oi) = args.min_oi {
        let min_oi_number = number_arg(min_oi as f64)?;
        contracts.retain(|contract| filter_by_oi(contract, min_oi_number));
        filters_applied.push(format!("oi >= {min_oi}"));
    }
    if let Some(max_spread_pct) = args.max_spread_pct {
        let max_spread_pct_number = number_arg(max_spread_pct)?;
        contracts.retain(|contract| filter_by_spread_pct(contract, max_spread_pct_number));
        filters_applied.push(format!("spreadPct <= {}", format_number(max_spread_pct)));
    }
    if args.min_premium.is_some() || args.max_premium.is_some() {
        let min = optional_number(args.min_premium)?;
        let max = optional_number(args.max_premium)?;
        contracts.retain(|contract| filter_by_premium(contract, min, max));
        if let Some(min_premium) = args.min_premium {
            filters_applied.push(format!("premium >= {}", format_number(min_premium)));
        }
        if let Some(max_premium) = args.max_premium {
            filters_applied.push(format!("premium <= {}", format_number(max_premium)));
        }
    }

    Ok(())
}

fn apply_sort(
    contracts: &mut [FlatContract],
    args: &ScreenArgs,
    filters_applied: &mut Vec<String>,
) -> Result<(), AppError> {
    let Some(sort) = args.sort.as_deref() else {
        return Ok(());
    };
    let spec = parse_sort_spec(sort)?;

    contracts.sort_by(|left, right| {
        let ordering = compare_values(
            &sort_value(left, spec.field),
            &sort_value(right, spec.field),
        );
        match spec.direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
    filters_applied.push(format!("sort = {}:{}", spec.field, spec.direction.as_str()));

    Ok(())
}

fn selected_fields(args: &ScreenArgs) -> Result<Vec<String>, AppError> {
    let fields = args.fields.as_deref().map_or_else(
        || {
            SCREEN_FIELDS
                .iter()
                .map(|field| (*field).to_string())
                .collect()
        },
        parse_fields,
    );
    let fields = if fields.is_empty() {
        SCREEN_FIELDS
            .iter()
            .map(|field| (*field).to_string())
            .collect()
    } else {
        fields
    };

    validate_fields(&fields)?;
    Ok(fields)
}

fn parse_fields(fields: &str) -> Vec<String> {
    fields
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_sort_spec(sort: &str) -> Result<SortSpec<'_>, AppError> {
    let (field, direction) = match sort.rsplit_once(':') {
        Some((field, direction)) => (field.trim(), Some(direction.trim())),
        None => (sort.trim(), None),
    };

    if !ALL_FIELDS.contains_key(field) {
        return Err(AppError::OptionsValidation {
            message: format!("unknown sort field: {field}"),
        });
    }

    let direction =
        direction.map_or_else(|| Ok(default_sort_direction(field)), SortDirection::parse)?;

    Ok(SortSpec { field, direction })
}

fn default_sort_direction(field: &str) -> SortDirection {
    match field {
        "bid" | "ask" | "mark" | "last" | "close" | "highPrice" | "lowPrice" | "gamma"
        | "theta" | "vega" | "rho" | "iv" | "volatility" | "oi" | "openInterest" | "volume"
        | "totalVolume" | "theoreticalValue" | "intrinsicValue" | "extrinsicValue"
        | "timeValue" | "multiplier" | "percentChange" | "markChange" | "markPercentChange" => {
            SortDirection::Desc
        }
        _ => SortDirection::Asc,
    }
}

fn sort_value(contract: &FlatContract, field: &str) -> Value {
    match field {
        "expiration" | "expiry" => Value::String(contract.expiration.clone()),
        "dte" => Value::from(contract.dte),
        "strike" => number_value(contract.strike),
        "type" => Value::String(contract.contract_type.clone()),
        "contract_type" | "cp" => Value::String(contract.contract_type.clone()),
        "symbol" => option_value(&contract.symbol),
        "description" => option_value(&contract.description),
        "bid" => option_value(&contract.bid),
        "ask" => option_value(&contract.ask),
        "mark" => option_value(&contract.mark),
        "last" => option_value(&contract.last),
        "close" => option_value(&contract.close),
        "highPrice" => option_value(&contract.high_price),
        "lowPrice" => option_value(&contract.low_price),
        "delta" => option_value(&contract.delta),
        "gamma" => option_value(&contract.gamma),
        "theta" => option_value(&contract.theta),
        "vega" => option_value(&contract.vega),
        "rho" => option_value(&contract.rho),
        "iv" | "volatility" => option_value(&contract.iv),
        "oi" | "openInterest" => option_value(&contract.oi),
        "volume" | "totalVolume" => option_value(&contract.volume),
        "itm" | "inTheMoney" => option_value(&contract.itm),
        "theoreticalValue" => option_value(&contract.theoretical_value),
        "intrinsicValue" => option_value(&contract.intrinsic_value),
        "extrinsicValue" => option_value(&contract.extrinsic_value),
        "timeValue" => option_value(&contract.time_value),
        "multiplier" => option_value(&contract.multiplier),
        "exerciseType" => option_value(&contract.exercise_type),
        "settlementType" => option_value(&contract.settlement_type),
        "expirationType" => option_value(&contract.expiration_type),
        "percentChange" => option_value(&contract.percent_change),
        "markChange" => option_value(&contract.mark_change),
        "markPercentChange" => option_value(&contract.mark_percent_change),
        "daysToExpiration" => option_value(&contract.days_to_expiration),
        _ => Value::Null,
    }
}

fn compare_values(left: &Value, right: &Value) -> Ordering {
    match (sort_key(left), sort_key(right)) {
        (SortKey::Null, SortKey::Null) => Ordering::Equal,
        (SortKey::Null, _) => Ordering::Greater,
        (_, SortKey::Null) => Ordering::Less,
        (SortKey::Number(left), SortKey::Number(right)) => left.total_cmp(&right),
        (SortKey::String(left), SortKey::String(right)) => left.cmp(&right),
        (SortKey::Bool(left), SortKey::Bool(right)) => left.cmp(&right),
        (left, right) => left.rank().cmp(&right.rank()),
    }
}

fn sort_key(value: &Value) -> SortKey<'_> {
    match value {
        Value::Null => SortKey::Null,
        Value::Bool(value) => SortKey::Bool(*value),
        Value::Number(value) => value.as_f64().map_or(SortKey::Null, SortKey::Number),
        Value::String(value) => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map_or(SortKey::String(value), SortKey::Number),
        _ => SortKey::Null,
    }
}

fn underlying_price(chain: &OptionChain) -> Value {
    chain
        .underlying_price
        .as_ref()
        .or_else(|| {
            chain
                .underlying
                .as_ref()
                .and_then(|underlying| underlying.last.as_ref())
        })
        .or_else(|| {
            chain
                .underlying
                .as_ref()
                .and_then(|underlying| underlying.mark.as_ref())
        })
        .and_then(|value| to_value(value).ok())
        .unwrap_or(Value::Null)
}

fn optional_number(value: Option<f64>) -> Result<Option<Number>, AppError> {
    value.map(number_arg).transpose()
}

fn number_arg(value: f64) -> Result<Number, AppError> {
    to_number(value).map_err(|error| AppError::OptionsValidation {
        message: error.to_string(),
    })
}

fn number_value(value: Number) -> Value {
    to_value(value).unwrap_or(Value::Null)
}

fn option_value(value: &Option<Value>) -> Value {
    value.clone().unwrap_or(Value::Null)
}

fn normalized_contract_type(contract_type: Option<&str>) -> Option<String> {
    contract_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_uppercase)
}

fn format_number(value: f64) -> String {
    let formatted = format!("{value}");
    formatted
}

#[derive(Clone, Copy)]
struct SortSpec<'a> {
    field: &'a str,
    direction: SortDirection,
}

#[derive(Clone, Copy)]
enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value.to_ascii_lowercase().as_str() {
            "asc" => Ok(Self::Asc),
            "desc" => Ok(Self::Desc),
            other => Err(AppError::OptionsValidation {
                message: format!("sort direction must be asc or desc, got {other}"),
            }),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

enum SortKey<'a> {
    Null,
    Bool(bool),
    Number(f64),
    String(&'a str),
}

impl SortKey<'_> {
    const fn rank(&self) -> u8 {
        match self {
            Self::Number(_) => 0,
            Self::String(_) => 1,
            Self::Bool(_) => 2,
            Self::Null => 3,
        }
    }
}
