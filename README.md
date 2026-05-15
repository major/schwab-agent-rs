# schwab-agent-rs

Agent-oriented JSON CLI porcelain for the Charles Schwab API, built on top of [schwab-rs](https://github.com/major/schwab-rs).

> **Disclaimer:** This project is unofficial and is not affiliated with, endorsed by, or connected to Charles Schwab, TD Ameritrade, or thinkorswim in any way. Use at your own risk.

[![CI](https://github.com/major/schwab-agent-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/major/schwab-agent-rs/actions/workflows/ci.yml)

| | |
|---|---|
| License | MIT |
| MSRV | 1.95 |
| Edition | 2024 |

## Overview

`schwab-agent` is a CLI binary that wraps the `schwab` crate and emits structured JSON for every command. It is designed for LLM agents and automation pipelines that need predictable, machine-readable output from the Schwab brokerage API.

All output uses a versioned `Envelope<T>` JSON wrapper:

```json
{
  "ok": true,
  "command": "market.quote",
  "version": 1,
  "data": { "..." : "..." },
  "warnings": [],
  "meta": { "..." : "..." }
}
```

Errors use the same envelope shape with an `error` field instead of `data`.

## Prerequisites

- Rust toolchain (stable, >= 1.95)
- A Charles Schwab developer application (client ID + secret)

## Building

```bash
cargo build --release
```

The `decimal` feature is enabled by default, switching price types to fixed-point decimals. To build without it:

```bash
cargo build --release --no-default-features
```

## Configuration

Set environment variables for authentication:

```bash
export SCHWAB_CLIENT_ID="your-client-id"
export SCHWAB_CLIENT_SECRET="your-client-secret"
# Token path defaults to $XDG_CONFIG_DIR/schwab-agent-rs/token.json
# Override with SCHWAB_TOKEN_PATH if needed
```

Global CLI flags (`--token`, `--client-id`, `--client-secret`, `--callback-url`) are also available.

### Mutable Operation Guard

All mutable commands (place, place-from-preview, place-raw, replace, cancel) are disabled by default. To enable them, set `"i-also-like-to-live-dangerously": true` in `~/.config/schwab-agent/config.json`:

```json
{
  "i-also-like-to-live-dangerously": true
}
```

This config file is shared with the Go CLI (`schwab-agent`). Missing config file or missing key defaults to disabled (safe default). Read-only commands (build, preview, list, get, quote, etc.) are not affected.

## Command Groups

### auth

Token management: `status`, `login`, `login-url`, `exchange`, `refresh`.

```bash
schwab-agent auth login-url       # get OAuth URL
schwab-agent auth exchange --redirect-url "https://..."
schwab-agent auth refresh         # refresh expired token
schwab-agent auth status          # check token state
```

### market

Market data: `quote`, `history`.

```bash
schwab-agent market quote AAPL MSFT
schwab-agent market history SPY --period 10 --period-type day
```

### account

Compact account discovery for LLM agents: `summary`, `resolve`.

Use `account summary` to list available account hashes and nicknames, then pass the chosen value to `--account` in stock and order commands.

```bash
schwab-agent account summary              # list all accounts with balances
schwab-agent account summary --positions  # include holdings
schwab-agent account resolve Trading      # resolve nickname to canonical hash
```

### portfolio

Account snapshot with optional positions.

```bash
schwab-agent portfolio snapshot --positions
```

### stock

Equity order workflow with four actions: `buy`, `sell`, `sell-short`, `buy-to-cover`.

Subcommands: `build`, `preview`, `place`, `place-from-preview`, `preview-raw`, `place-raw`.

Each action hardcodes the Schwab `Instruction` to prevent accidental trade reversal.

```bash
schwab-agent stock build buy AAPL --quantity 10 --price 150.00
schwab-agent stock preview buy AAPL --quantity 10 --price 150.00 --account HASH --save-preview
schwab-agent stock place-from-preview --account HASH_OR_NICKNAME --digest DIGEST_HEX
```

### order

Option order workflow supporting 15 named strategies: `long-call`, `long-put`, `cash-secured-put`, `naked-call`, `sell-covered-call`, `call-debit-spread`, `call-credit-spread`, `put-debit-spread`, `put-credit-spread`, `long-straddle`, `short-straddle`, `long-strangle`, `short-strangle`, `short-iron-condor`, `jade-lizard`.

Subcommands: `build`, `preview`, `place`, `place-from-preview`, `replace`, `list`, `get`, `cancel`.

Each strategy hardcodes contract type and direction to prevent accidental trade reversal.

Lifecycle commands (`list`, `get`, `replace`, `cancel`) manage existing orders across accounts. `replace` builds a new option strategy payload and submits it to Schwab's replace endpoint for an existing order ID.

```bash
schwab-agent order replace --account HASH 12345678 long-call AAPL --expiration 2025-06-20 --strike 200 --price 5.50
```

### option

Option chain data: `expirations`, `chain`, `screen`, `contract`.

```bash
schwab-agent option expirations AAPL
schwab-agent option chain AAPL --expiration 2025-06-20 --type CALL
schwab-agent option screen AAPL --expiration 2025-06-20 --delta-min 0.20 --delta-max 0.40
schwab-agent option contract AAPL --expiration 2025-06-20 --strike 200 --call
```

### ta

Technical analysis: `dashboard`, `expected-move`.

```bash
schwab-agent ta dashboard AAPL                          # daily TA dashboard, 20 data points
schwab-agent ta dashboard SPY --interval weekly --points 10
schwab-agent ta expected-move AAPL                      # expected move from ATM straddle
schwab-agent ta expected-move SPY --dte 45              # 45-day expected move
```

Dashboard flags: `--interval` (daily, weekly, 1min, 5min, 15min, 30min; default: daily), `--points` (number of data points; default: 20).
Expected-move flags: `--dte` (days to expiration; default: 30).

### analyze

Multi-symbol analysis with partial-failure support.

```bash
schwab-agent analyze AAPL                    # single symbol
schwab-agent analyze AAPL MSFT GOOG SPY      # multiple symbols
schwab-agent analyze AAPL --interval weekly --points 10
```

Returns quote + TA dashboard for each symbol. If some symbols fail, the envelope still returns `ok: true` with partial data and warnings for failed symbols. All symbols failing produces `ok: false`.

## Order Workflow

The recommended agent workflow uses tamper-evident previews:

1. `preview --save-preview` - preview the order and save to disk
2. `place-from-preview` - submit the exact saved payload after SHA-256 digest, 15-minute TTL, and account checks pass

Direct `place` is available for explicit human use, but agents should prefer the preview workflow.

Previews are stored in `$XDG_STATE_DIR/schwab-agent/previews/`.

### Post-Action Verification

All mutable order actions (place, place-from-preview, place-raw, replace, cancel) automatically follow up with a GET to retrieve the order status. Schwab's API only returns a Location header and order ID on placement and replacement, so the CLI verifies by fetching the full order. The response preserves the existing `order_id`, `location`, and submitted `order` fields, and adds `verification_state`, optional `verification_failures`, and `verified_order` when the follow-up GET returns order details.

`order list --from` and `--to` accept either date-only values (`YYYY-MM-DD`) or exact RFC3339 instants. Date-only ranges are interpreted as inclusive UTC calendar days, so `--from 2026-05-28 --to 2026-05-31` searches from `2026-05-28T00:00:00Z` through `2026-05-31T23:59:59.999999999Z`.

## Testing

```bash
make check    # fmt + clippy + test + doc (runs both default and decimal feature configs)
make test     # tests only (default + decimal)
make coverage # cargo llvm-cov, 90% line coverage threshold
make audit    # cargo audit
```

CI runs on Ubuntu, macOS, and Windows with MSRV verification against 1.95.

## License

MIT
