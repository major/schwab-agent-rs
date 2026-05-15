use std::collections::BTreeSet;

use schwab::{Client, Number, OptionChain, OptionChainOptions};
use serde_json::{Value, json, to_value};
use time::{Date, Duration, OffsetDateTime};

use crate::cli::ChainArgs;
use crate::error::AppError;

use super::types::{
    CHAIN_FIELDS, FlatContract, compute_dte, filter_by_delta, filter_by_strike, flatten_chain,
    select_fields, sort_contracts, validate_fields,
};

/// Fetches, filters, and projects a compact option chain response.
pub async fn handle(client: &Client, args: &ChainArgs) -> Result<Value, AppError> {
    let options = chain_options(args);
    let chain = client
        .get_option_chain(options)
        .await
        .map_err(|error| map_chain_error(error, &args.symbol))?;

    render_chain(&chain, args)
}

pub(super) fn render_chain(chain: &OptionChain, args: &ChainArgs) -> Result<Value, AppError> {
    let underlying_price = chain.underlying_price.or_else(|| {
        chain
            .underlying
            .as_ref()
            .and_then(|underlying| underlying.mark)
    });
    let mut contracts = flatten_chain(chain);
    sort_contracts(&mut contracts);
    apply_filters(&mut contracts, args)?;

    let requested_fields = requested_fields(args)?;
    let field_refs = requested_fields
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (columns, rows) = select_fields(&contracts, &field_refs);

    Ok(json!({
        "underlying": args.symbol,
        "underlyingPrice": underlying_price.map(|price| to_value(price).unwrap_or_default()),
        "columns": columns,
        "rows": rows,
        "rowCount": rows.len(),
    }))
}

fn chain_options(args: &ChainArgs) -> OptionChainOptions {
    let mut options = OptionChainOptions::new(&args.symbol)
        .parameter("strategy", "SINGLE")
        .include_underlying_quote(true);

    if let Some(contract_type) = &args.contract_type {
        options = options.parameter("contractType", contract_type.to_uppercase());
    }
    if let Some(strike_count) = args.strike_count {
        options = options.integer_parameter("strikeCount", i64::from(strike_count));
    }
    if let Some(strike_range) = &args.strike_range {
        options = options.parameter("range", strike_range);
    }
    if let Some(strike) = args.strike {
        options = options.number_parameter("strike", strike);
    }
    if let Some(dte) = args.dte {
        let target = OffsetDateTime::now_utc()
            .date()
            .saturating_add(Duration::days(i64::from(dte)));
        options = options
            .parameter(
                "fromDate",
                date_string(target.saturating_sub(Duration::days(1))),
            )
            .parameter(
                "toDate",
                date_string(target.saturating_add(Duration::days(1))),
            );
    }
    if let Some(expiration) = &args.expiration {
        options = options
            .parameter("fromDate", expiration)
            .parameter("toDate", expiration);
    }

    options
}

fn apply_filters(contracts: &mut Vec<FlatContract>, args: &ChainArgs) -> Result<(), AppError> {
    if let Some(contract_type) = &args.contract_type {
        let contract_type = contract_type.to_uppercase();
        if contract_type != "ALL" {
            contracts.retain(|contract| contract.contract_type == contract_type);
        }
    }

    if let Some(expiration) = &args.expiration {
        contracts.retain(|contract| contract.expiration == *expiration);
    }

    if let Some(dte) = args.dte {
        if let Some(expiration) = nearest_expiration(contracts, dte) {
            contracts.retain(|contract| contract.expiration == expiration);
        } else {
            contracts.clear();
        }
    }

    if args.strike_min.is_some() || args.strike_max.is_some() {
        let min = optional_number(args.strike_min)?;
        let max = optional_number(args.strike_max)?;
        contracts.retain(|contract| filter_by_strike(contract, min, max, None));
    }

    if args.delta_min.is_some() || args.delta_max.is_some() {
        let min = optional_number(args.delta_min)?;
        let max = optional_number(args.delta_max)?;
        contracts.retain(|contract| filter_by_delta(contract, min, max));
    }

    Ok(())
}

fn nearest_expiration(contracts: &[FlatContract], target_dte: i32) -> Option<String> {
    contracts
        .iter()
        .map(|contract| contract.expiration.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|expiration| compute_dte(expiration).map(|dte| (expiration, dte)))
        .min_by(
            |(left_expiration, left_dte), (right_expiration, right_dte)| {
                (left_dte - target_dte)
                    .abs()
                    .cmp(&(right_dte - target_dte).abs())
                    .then_with(|| left_expiration.cmp(right_expiration))
            },
        )
        .map(|(expiration, _)| expiration.to_string())
}

fn requested_fields(args: &ChainArgs) -> Result<Vec<String>, AppError> {
    let fields = args.fields.as_ref().map_or_else(
        || {
            CHAIN_FIELDS
                .iter()
                .map(|field| (*field).to_string())
                .collect::<Vec<_>>()
        },
        |fields| {
            fields
                .split(',')
                .map(str::trim)
                .filter(|field| !field.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        },
    );
    validate_fields(&fields)?;
    Ok(fields)
}

fn optional_number(value: Option<f64>) -> Result<Option<Number>, AppError> {
    value
        .map(|value| serde_json::from_value(json!(value)).map_err(AppError::from))
        .transpose()
}

fn map_chain_error(error: schwab::Error, symbol: &str) -> AppError {
    match error {
        schwab::Error::HttpStatus { status, .. } if status == 400 || status == 404 => {
            AppError::OptionsSymbolNotFound {
                symbol: symbol.to_string(),
            }
        }
        error => AppError::Schwab(error),
    }
}

fn date_string(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}
