# schwab-agent CLI

Structured JSON CLI for Charles Schwab API. All output is JSON envelopes. Set env vars once, then most commands need zero flags.

> **Disclaimer:** This project is unofficial and is not affiliated with, endorsed by, or connected to Charles Schwab, TD Ameritrade, or thinkorswim in any way.

## Setup

```bash
export SCHWAB_CLIENT_ID="..."
export SCHWAB_CLIENT_SECRET="..."
# Token path defaults to $XDG_CONFIG_DIR/schwab-agent-rs/token.json
# Callback URL defaults to https://127.0.0.1:8182
```

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
schwab-agent market history SPY             # price history (defaults are fine)
```

Optional history flags: `--period-type`, `--period`, `--frequency-type`, `--frequency`, `--from`, `--to`, `--extended-hours`.

## Portfolio

```bash
schwab-agent portfolio snapshot --account HASH
schwab-agent portfolio snapshot --account HASH --positions   # include holdings
```

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
schwab-agent order cancel --account HASH 12345678                # cancel + verify
```

List flags: `--account` (optional), `--status`, `--from`/`--to` (`YYYY-MM-DD` or RFC3339), `--recent`, `--max-results`. Date-only ranges are inclusive UTC calendar days, so `--from 2026-05-28 --to 2026-05-31` includes both end dates and the dates between them. Output: `{"orders": [...], "count": N}`.

## Post-Place Verification

All mutable actions (place, place-from-preview, place-raw, cancel) auto-verify by GETting the order after the action. Schwab only returns a Location header on placement, so this GET is what gives the LLM actual order state.

Response `data` fields: `action` ("place"/"cancel"), `order_id`, `location`, `order` (submitted payload), `verification_state` ("verified"/"unverified"), and `verified_order` (full order from GET when available). Optional: `verification_failures` (when unverified), `digest`/`original_command` (for place-from-preview). Unverified failures appear in the envelope `warnings` array; the order may still have succeeded. Cancel verification is only `verified` when the fetched order status is `CANCELED`.

## Option Orders

Recommended LLM workflow: `preview --save-preview` (with account) -> `place-from-preview` (with digest). This places the exact saved preview payload after the digest, TTL, and account checks pass.

`build` is an optional local dry run for inspecting order JSON. Direct `place` is available for explicit human requests, but LLM agents should prefer saved previews so the submitted payload cannot drift from the reviewed preview.

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

## Output Format

Every response is a JSON envelope:

```json
{"ok": true, "command": "market.quote", "version": 1, "data": {...}, "warnings": [], "meta": {"generated_at": "..."}}
```

Errors:

```json
{"ok": false, "command": "auth.status", "version": 1, "error": {"code": "auth.token_missing", "message": "...", "category": "auth", "retryable": false, "hint": "..."}}
```

Check `ok` first. On error, read `error.hint` for recovery steps. Check `error.retryable` before retrying.

### Error Codes

| Code | Meaning | Recovery |
|---|---|---|
| `auth.config_missing` | No client ID/secret | Set `SCHWAB_CLIENT_ID` and `SCHWAB_CLIENT_SECRET` |
| `auth.token_missing` | No token file | Run `auth login-url` then `auth exchange` |
| `auth.expired` | Token expired | Run `auth refresh` |
| `auth.required` | Auth needed | Run full auth flow |
| `schwab.http_status` | API HTTP error | Check message for status code |
| `input.empty_symbols` | No symbols given | Provide at least one symbol |
| `order.validation_failed` | Bad order params | Check strike/expiration values |
| `order.preview_failed` | Preview issue | Re-run preview (may have expired) |
| `options.symbol_not_found` | Symbol has no options | Verify symbol is optionable |
| `options.validation_failed` | Invalid option params | Check expiration/strike values |
