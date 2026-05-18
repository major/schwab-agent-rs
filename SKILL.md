# schwab-agent CLI

Structured JSON CLI for Charles Schwab API. All output is raw JSON data payloads. Set env vars once, then most commands need zero flags.

> **Disclaimer:** This project is unofficial and is not affiliated with, endorsed by, or connected to Charles Schwab, TD Ameritrade, or thinkorswim in any way.

## Setup

```bash
export SCHWAB_CLIENT_ID="..."
export SCHWAB_CLIENT_SECRET="..."
# Token path defaults to $XDG_CONFIG_DIR/schwab-agent-rs/token.json
# Callback URL defaults to https://127.0.0.1:8182
```

## Release Notes

This crate is published as `schwab-agent-rs`. Releases are automated on push to main: release-plz creates release PRs from Conventional Commits, publishes to crates.io with Trusted Publishing, and creates git tags. cargo-dist then builds cross-platform binaries and creates GitHub Releases with shell and PowerShell installers.

## Mutable Operation Guard

All mutable commands (place, place-from-preview, place-raw, replace, cancel) require `"i-also-like-to-live-dangerously": true` in `~/.config/schwab-agent/config.json`. Without it, these commands return error code `config.mutable_disabled` (exit code 10). Read-only commands (build, preview, list, get) are not gated.

## Auth

```bash
schwab-agent auth status          # check token state
schwab-agent auth login-url       # get OAuth URL (open in browser)
schwab-agent auth exchange --redirect-url "CALLBACK_URL_WITH_CODE"
schwab-agent auth refresh         # refresh expired token
schwab-agent auth login           # interactive: opens browser, waits for callback
```

If you get `auth.token_missing`, run `login-url` then `exchange`. If `auth.expired`, run `refresh`.

## Market Data

```bash
schwab-agent market quote AAPL              # single quote
schwab-agent market quote AAPL MSFT GOOG    # multiple quotes
schwab-agent market quote AAPL --fields sym,last,pct,vol
schwab-agent market quote AAPL --all-fields
schwab-agent market history SPY             # price history (defaults are fine)
schwab-agent market history SPY --fields ts,close,vol
schwab-agent market history SPY --all-fields
```

Quote output defaults to token-efficient rows: `columns`, `rows`, and `rowCount`. Default columns are `req`, `sym`, `bid`, `ask`, `last`, `mark`, `chg`, `pct`, `vol`, and `err` so per-symbol quote errors stay visible in compact output. Use `--fields` for specific output columns, using compact names or full aliases such as `requested_symbol`, `symbol`, `net_change`, `net_percent_change`, `volume`, and `error`. Use `--all-fields` for full detailed quote objects. Use `--api-fields quote,reference` only to limit Schwab API field groups.

History output also defaults to token-efficient rows with `symbol`, `columns`, `rows`, and `rowCount`. Default candle columns are `ts`, `open`, `high`, `low`, `close`, and `vol`, which are enough for most trading decisions and TA handoffs. Use `--fields` for specific candle columns, using compact names or aliases such as `timestamp`, `datetime`, `datetimeISO8601`, `iso`, `o`, `h`, `l`, `c`, and `volume`. Use `--all-fields` for the full Schwab price history object, including previous-close metadata and raw candle objects.

Optional history flags: `--period-type`, `--period`, `--frequency-type`, `--frequency`, `--from`, `--to`, `--extended-hours`.

## Account

Discover and resolve accounts before placing orders.

Recommended workflow: `account summary` -> choose `account_hash` or nickname -> pass to `--account` in stock/order commands.

```bash
schwab-agent account summary                                    # list accounts with balances
schwab-agent account summary --positions                        # include holdings (default compact columns)
schwab-agent account summary --positions --fields sym,mktval,pnl  # select position columns
schwab-agent account summary --positions --all-fields           # all 9 curated position fields as objects
schwab-agent account summary --with-positions-only              # only accounts that hold positions
schwab-agent account resolve Trading                            # resolve nickname to canonical hash
schwab-agent account resolve ABCDEF1234567890                   # verify a known hash
```

Position output with `--positions` is token-optimized by default, returning `columns`, `rows`, and `rowCount` per account. Default columns are `sym`, `long_qty`, `avg`, `mktval`, `pnl`, and `pnlpct`. Use `--fields` to select position columns by compact names or full aliases such as `symbol`, `description`, `asset_type`, `long_quantity`, `short_quantity`, `average_price`, `market_value`, `current_day_profit_loss`, and `current_day_profit_loss_percentage`. Use `--all-fields` for curated compact position objects with all 9 fields. Both `--fields` and `--all-fields` require `--positions`.

The `--account` flag on stock and order commands accepts either the canonical account hash or a unique nickname. Raw account numbers are not supported.


## Stock Orders

Buy/sell shares of stock. Recommended LLM workflow: `preview --save-preview` -> `place-from-preview` (same digest/TTL system as option orders).

Prefer limit orders when practical: pass `--price` for limit orders. Omit `--price` only when a market order is explicitly desired.

### Buy / Sell

```bash
# Market order (default)
schwab-agent stock build buy AAPL --quantity 10
schwab-agent stock build sell AAPL --quantity 10

# Limit order
schwab-agent stock build buy AAPL --quantity 10 --order-type limit --price 180.00

# Stop order
schwab-agent stock build buy AAPL --quantity 10 --order-type stop --stop-price 170.00

# Stop-limit order
schwab-agent stock build buy AAPL --quantity 10 --order-type stop-limit --price 169.00 --stop-price 170.00
```

### Short Selling

```bash
schwab-agent stock build sell-short AAPL --quantity 10 --order-type limit --price 200.00
schwab-agent stock build buy-to-cover AAPL --quantity 10 --order-type limit --price 180.00
```

### Stock Preview and Place

```bash
# Preview and save digest
schwab-agent stock preview --account HASH --save-preview buy AAPL --quantity 100 --order-type limit --price 180.00

# Place from saved preview (15-min TTL)
schwab-agent stock place-from-preview --account HASH --digest DIGEST_HEX

# Direct place (for explicit human requests only)
schwab-agent stock place --account HASH buy AAPL --quantity 100 --order-type limit --price 180.00
```

### Complex Orders (Bracket, OCO, Trigger)

Use `preview-raw` and `place-raw` to submit arbitrary JSON payloads for order types that aren't covered by the porcelain commands. This is the path for bracket orders, OCO (one-cancels-other), and triggered orders.

#### Bracket Order (Buy + Stop Loss + Profit Target)

A bracket order is a `TRIGGER` parent with two `OCO` child orders. When the parent fills, both children activate; when one child fills, the other cancels.

```bash
schwab-agent stock preview-raw --account HASH --save-preview --json '{
  "orderType": "LIMIT",
  "session": "NORMAL",
  "duration": "DAY",
  "orderStrategyType": "TRIGGER",
  "price": "180.00",
  "orderLegCollection": [
    {
      "instruction": "BUY",
      "quantity": 100,
      "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
    }
  ],
  "childOrderStrategies": [
    {
      "orderStrategyType": "OCO",
      "childOrderStrategies": [
        {
          "orderType": "LIMIT",
          "session": "NORMAL",
          "duration": "GOOD_TILL_CANCEL",
          "orderStrategyType": "SINGLE",
          "price": "200.00",
          "orderLegCollection": [
            {
              "instruction": "SELL",
              "quantity": 100,
              "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
            }
          ]
        },
        {
          "orderType": "STOP",
          "session": "NORMAL",
          "duration": "GOOD_TILL_CANCEL",
          "orderStrategyType": "SINGLE",
          "stopPrice": "170.00",
          "orderLegCollection": [
            {
              "instruction": "SELL",
              "quantity": 100,
              "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
            }
          ]
        }
      ]
    }
  ]
}'
```

#### OCO Order (Stop Loss OR Profit Target)

An OCO order places two orders where filling one cancels the other. Use this when you already hold shares and want to set both a stop loss and a profit target.

```bash
schwab-agent stock place-raw --account HASH --json '{
  "orderStrategyType": "OCO",
  "childOrderStrategies": [
    {
      "orderType": "LIMIT",
      "session": "NORMAL",
      "duration": "GOOD_TILL_CANCEL",
      "orderStrategyType": "SINGLE",
      "price": "200.00",
      "orderLegCollection": [
        {
          "instruction": "SELL",
          "quantity": 100,
          "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
        }
      ]
    },
    {
      "orderType": "STOP",
      "session": "NORMAL",
      "duration": "GOOD_TILL_CANCEL",
      "orderStrategyType": "SINGLE",
      "stopPrice": "170.00",
      "orderLegCollection": [
        {
          "instruction": "SELL",
          "quantity": 100,
          "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
        }
      ]
    }
  ]
}'
```

#### Triggered Order (Buy, Then Stop Loss)

A `TRIGGER` parent fires its child orders when the parent fills. Use this when you want a stop loss activated automatically after a buy.

```bash
schwab-agent stock place-raw --account HASH --json '{
  "orderType": "LIMIT",
  "session": "NORMAL",
  "duration": "DAY",
  "orderStrategyType": "TRIGGER",
  "price": "180.00",
  "orderLegCollection": [
    {
      "instruction": "BUY",
      "quantity": 100,
      "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
    }
  ],
  "childOrderStrategies": [
    {
      "orderType": "STOP",
      "session": "NORMAL",
      "duration": "GOOD_TILL_CANCEL",
      "orderStrategyType": "SINGLE",
      "stopPrice": "170.00",
      "orderLegCollection": [
        {
          "instruction": "SELL",
          "quantity": 100,
          "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
        }
      ]
    }
  ]
}'
```

#### Key Fields for Complex Orders

- `orderStrategyType`: `"SINGLE"` (leaf), `"TRIGGER"` (parent fires children on fill), `"OCO"` (one-cancels-other)
- `childOrderStrategies`: Array of child orders (recursive structure)
- `instruction`: `"BUY"`, `"SELL"`, `"BUY_TO_COVER"`, `"SELL_SHORT"`
- `orderType`: `"MARKET"`, `"LIMIT"`, `"STOP"`, `"STOP_LIMIT"`, `"TRAILING_STOP"`
- Prices are strings in raw JSON (e.g., `"180.00"` not `180.00`)

## Order Lifecycle

```bash
schwab-agent order list                                          # all accounts, last 60 days
schwab-agent order list --account HASH --recent                  # single account, last 24h
schwab-agent order list --account HASH --status WORKING --from 2025-01-01 --to 2025-01-31
schwab-agent order get --account HASH 12345678                   # single order by ID
schwab-agent order replace --account HASH 12345678 long-call AAPL --expiration 2025-06-20 --strike 200 --price 5.50
schwab-agent order cancel --account HASH 12345678                # cancel + verify
```

List flags: `--account` (optional), `--status`, `--from`/`--to` (`YYYY-MM-DD` or RFC3339), `--recent`, `--max-results`. Date-only ranges are inclusive UTC calendar days, so `--from 2026-05-28 --to 2026-05-31` includes both end dates and the dates between them. Output: `{"orders": [...], "count": N}`.

## Post-Action Verification

All mutable actions (place, place-from-preview, place-raw, replace, cancel) auto-verify by GETting the order after the action. Schwab only returns a Location header on placement and replacement, so this GET is what gives the LLM actual order state.

Response fields: `action` ("place"/"replace"/"cancel"), `order_id`, `location`, `order` (submitted payload), `verification_state` ("verified"/"unverified"), and `verified_order` (full order from GET when available). Optional: `verification_failures` (when unverified), `digest`/`original_command` (for place-from-preview). Unverified failures are included in the response; the order may still have succeeded. Cancel verification is only `verified` when the fetched order status is `CANCELED`.

## Option Orders

Recommended LLM workflow: `preview --save-preview` (with account) -> `place-from-preview` (with digest). This places the exact saved preview payload after the digest, TTL, and account checks pass.

`build` is an optional local dry run for inspecting order JSON. Direct `place` is available for explicit human requests, but LLM agents should prefer saved previews so the submitted payload cannot drift from the reviewed preview.

`replace` rebuilds a safe strategy payload and submits it for an existing order ID, then verifies the resulting order with a follow-up GET. It does not use the preview digest ledger.

Prefer limit-style pricing whenever practical: pass `--price` so single-leg orders use `LIMIT` and multi-leg orders use `NET_DEBIT` or `NET_CREDIT`. Omit `--price` only when a market order is explicitly desired.

### Single-Leg

```bash
# Required: UNDERLYING --expiration YYYY-MM-DD --strike PRICE
# Defaults: --quantity 1, --session normal, --duration day
schwab-agent order build long-call AAPL --expiration 2025-06-20 --strike 200 --price 5.00
schwab-agent order preview --account HASH --save-preview long-put SPY --expiration 2025-06-20 --strike 550 --price 4.50
schwab-agent order place-from-preview --account HASH --digest DIGEST_HEX
```

Strategies: `long-call`, `long-put`, `cash-secured-put`, `naked-call`, `sell-covered-call`

### Vertical Spreads

```bash
# Uses --low-strike and --high-strike instead of --strike
schwab-agent order build put-credit-spread SPY --expiration 2025-06-20 --low-strike 540 --high-strike 550 --price 2.50
```

Strategies: `put-credit-spread`, `call-credit-spread`, `put-debit-spread`, `call-debit-spread`

### Straddles

```bash
# Same args as single-leg (one --strike for both legs)
schwab-agent order build long-straddle SPY --expiration 2025-06-20 --strike 550 --price 10.00
```

Strategies: `long-straddle`, `short-straddle`

### Strangles

```bash
# Uses --call-strike and --put-strike
schwab-agent order build long-strangle SPY --expiration 2025-06-20 --call-strike 560 --put-strike 540 --price 8.00
```

Strategies: `long-strangle`, `short-strangle`

### Iron Condor

```bash
schwab-agent order build short-iron-condor SPY --expiration 2025-06-20 \
  --put-long-strike 530 --put-short-strike 540 --call-short-strike 560 --call-long-strike 570 --price 2.00
```

### Jade Lizard

```bash
schwab-agent order build jade-lizard SPY --expiration 2025-06-20 \
  --put-strike 540 --short-call-strike 560 --long-call-strike 570 --price 3.00
```

### Preview and Place from Preview

```bash
# Save a preview digest for later execution
schwab-agent order preview --account HASH --save-preview long-call AAPL --expiration 2025-06-20 --strike 200 --price 5.00
# Place using saved preview (15-min TTL)
schwab-agent order place-from-preview --account HASH --digest DIGEST_HEX
```

### Duration Aliases

`day` (default), `good-till-cancel`/`gtc`, `fill-or-kill`/`fok`, `immediate-or-cancel`/`ioc`

## Option Data

Read-only option chain commands for research and strategy selection. No orders are placed. Recommended workflow: `expirations` to pick a date, `chain` to scan strikes, `contract` for a single contract's full detail. Use `screen` when you need multi-criteria filtering across expirations and strikes.

### Expirations

```bash
schwab-agent-rs option expirations AAPL
```

Returns a row-based list of available expiration dates for the underlying. Use the dates here as input to `--expiration` in `chain`, `screen`, and `contract`.

### Chain

```bash
# Full chain (all expirations, all strikes)
schwab-agent-rs option chain AAPL

# Calls near 30 DTE with selected fields
schwab-agent-rs option chain AAPL --type call --dte 30 --fields strike,delta,bid,ask,volume,oi

# Puts in a strike range with delta filter
schwab-agent-rs option chain AMD --type put --strike-min 140 --strike-max 160 --delta-min -0.30 --delta-max -0.15

# Exact expiration, specific strike count around ATM
schwab-agent-rs option chain SPY --expiration 2025-06-20 --strike-count 10
```

Chain flags:

| Flag | Description |
|---|---|
| `--type call\|put\|all` | Contract type filter (default: all) |
| `--dte N` | Nearest expiration by days to expiration |
| `--expiration YYYY-MM-DD` | Exact expiration date |
| `--delta-min N` | Minimum delta filter |
| `--delta-max N` | Maximum delta filter |
| `--fields LIST` | Comma-separated field list |
| `--strike-count N` | Strikes around at-the-money |
| `--strike N` | Exact strike price |
| `--strike-min N` | Minimum strike price |
| `--strike-max N` | Maximum strike price |
| `--strike-range RANGE` | Schwab strike range filter |

Output is row-based: `{ "columns": [...], "rows": [[...], ...], "rowCount": N }`.

### Screen

Screen adds liquidity and pricing filters on top of all chain flags. Use it when you want to narrow results by volume, open interest, spread quality, or premium range.

```bash
# Liquid calls with tight spreads, 20-45 DTE
schwab-agent-rs option screen AAPL --type call --dte-min 20 --dte-max 45 --min-volume 100 --min-oi 500 --max-spread-pct 10

# Premium range filter with result limit
schwab-agent-rs option screen SPY --type put --min-premium 1.00 --max-premium 5.00 --limit 20
```

Screen-only flags (all chain flags also apply):

| Flag | Description |
|---|---|
| `--dte-min N` | Minimum days to expiration |
| `--dte-max N` | Maximum days to expiration |
| `--min-bid N` | Minimum bid price |
| `--max-ask N` | Maximum ask price |
| `--min-volume N` | Minimum volume |
| `--min-oi N` | Minimum open interest |
| `--max-spread-pct N` | Maximum spread percent |
| `--min-premium N` | Minimum premium |
| `--max-premium N` | Maximum premium |
| `--sort FIELD` | Sort field |
| `--limit N` | Maximum number of results |

Output adds `totalScanned` and `filtersApplied` alongside the row-based data.

### Contract

Look up a single contract by expiration, strike, and type. Returns a flat object (no columns/rows).

```bash
schwab-agent-rs option contract AAPL --expiration 2025-06-20 --strike 200 --call
schwab-agent-rs option contract SPY --expiration 2025-06-20 --strike 550 --put
```

All three flags are required: `--expiration YYYY-MM-DD`, `--strike N`, and one of `--call` or `--put`.

## Technical Analysis

Read-only TA commands. No orders are placed.

### Dashboard

Runs all indicators for a symbol and returns category-grouped output: trend, momentum, volatility, and volume. Includes derived fields (ATR percent, relative volume, distance from SMAs) and signal interpretations.

```bash
schwab-agent ta dashboard AAPL                          # daily dashboard, 20 points
schwab-agent ta dashboard SPY --interval weekly --points 10
```

Dashboard flags:

| Flag | Description |
|---|---|
| `--interval INTERVAL` | Candle interval: daily (default), weekly, 1min, 5min, 15min, 30min |
| `--points N` | Number of data points per indicator series (default: 20) |

### Expected Move

Computes expected move from the ATM straddle price in the option chain. Output includes straddle price, expected move (price and percent), upper/lower ranges, and implied volatility from ATM options.

```bash
schwab-agent ta expected-move AAPL                      # 30-day expected move
schwab-agent ta expected-move SPY --dte 45
```

Expected-move flags:

| Flag | Description |
|---|---|
| `--dte N` | Target days to expiration for the option chain (default: 30) |

## Analyze

Multi-symbol analysis combining quote and TA dashboard per symbol. Partial failures include per-symbol error fields (`quote_error`, `analysis_error`) alongside successful results.

```bash
schwab-agent analyze AAPL                    # single symbol
schwab-agent analyze AAPL MSFT GOOG          # multiple symbols
schwab-agent analyze AAPL --interval weekly --points 10
```

Analyze flags:

| Flag | Description |
|---|---|
| `--interval INTERVAL` | Candle interval (same values as ta dashboard) |
| `--points N` | Number of data points per indicator series (default: 1) |

## Output Format

Commands output raw JSON data payloads directly (no wrapper envelope). Errors output a structured JSON object:

```json
{"code": "auth.token_missing", "message": "...", "category": "auth", "retryable": false, "hint": "..."}
```

On error (non-zero exit code), read `hint` for recovery steps. Check `retryable` before retrying.

### Error Codes

| Code | Meaning | Recovery |
|---|---|---|
| `auth.config_missing` | No client ID/secret | Add to `~/.config/schwab-agent/config.json` or set `SCHWAB_CLIENT_ID`/`SCHWAB_CLIENT_SECRET` |
| `auth.token_missing` | No token file | Run `auth login-url` then `auth exchange` |
| `auth.expired` | Token expired | Run `auth refresh` |
| `auth.required` | Auth needed | Run full auth flow |
| `schwab.http_status` | API HTTP error | Check message for status code |
| `input.empty_symbols` | No symbols given | Provide at least one symbol |
| `account.validation_failed` | Account input validation error | Read the error message and hint for details (invalid `--fields`, unknown account selector, ambiguous nickname) |
| `market.validation_failed` | Invalid market-data params | Use a listed `--fields` value or read the error hint |
| `order.validation_failed` | Bad order params | Check strike/expiration values |
| `order.preview_failed` | Preview issue | Re-run preview (may have expired) |
| `options.symbol_not_found` | Symbol has no options | Verify symbol is optionable |
| `options.validation_failed` | Invalid option params | Check expiration/strike values |
| `ta.insufficient_data` | Not enough candle data | Try a shorter interval or fewer points |
| `ta.invalid_interval` | Unrecognized interval | Use: daily, weekly, 1min, 5min, 15min, 30min |
| `config.mutable_disabled` | Mutable ops disabled | Set `"i-also-like-to-live-dangerously": true` in config |
| `ta.calculation_error` | Indicator math failed | Check input data quality |
