# schwab-agent-rs

Agent-oriented JSON CLI porcelain for the Charles Schwab API, built on top of [schwab-rs](https://github.com/major/schwab-rs).

> **Disclaimer:** This project is unofficial and is not affiliated with, endorsed by, or connected to Charles Schwab, TD Ameritrade, or thinkorswim in any way. Use at your own risk.

[![CI](https://github.com/major/schwab-agent-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/major/schwab-agent-rs/actions/workflows/ci.yml)

| | |
|---|---|
| License | MIT |
| MSRV | 1.95 |
| Edition | 2024 |
| Crate | [`schwab-agent-rs`](https://crates.io/crates/schwab-agent-rs) |

## Overview

`schwab-agent` is a CLI binary that wraps the `schwab` crate and emits structured JSON for every command. It is designed for LLM agents and automation pipelines that need predictable, machine-readable output from the Schwab brokerage API.

Commands output raw JSON data payloads directly for minimal token overhead. Errors output a structured JSON object with `code`, `message`, `category`, `retryable`, and `hint` fields.

Account response shape mismatches return `account.response_shape` with sanitized top-level JSON shape metadata in the message, so unexpected Schwab envelopes can be reported without exposing account numbers, balances, positions, or hashes.

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

Set environment variables for authentication, or add the same values to `~/.config/schwab-agent/config.json`:

```bash
export SCHWAB_CLIENT_ID="your-client-id"
export SCHWAB_CLIENT_SECRET="your-client-secret"
export SCHWAB_CALLBACK_URL="https://127.0.0.1:8182"
# Token path defaults to $XDG_CONFIG_DIR/schwab-agent-rs/token.json
# Override with SCHWAB_TOKEN_PATH if needed
```

### Mutable Operation Guard

All commands that submit, replace, repeat-place, or cancel orders are disabled by default. To enable them, set `"i-also-like-to-live-dangerously": true` in `~/.config/schwab-agent/config.json`:

```json
{
  "i-also-like-to-live-dangerously": true
}
```

This config file is shared with the Go CLI (`schwab-agent`). Missing config file or missing key defaults to disabled (safe default). Read-only commands (build, preview, get, quote, etc.) are not affected. `order repeat --save-preview` only previews and saves a digest, so it remains available without the mutable guard; direct repeat placement and `--preview-first` are gated.

## Command Groups

### auth

Token management: `status`, `login`, `login-url`, `exchange`, `refresh`.

```bash
schwab-agent auth login           # interactive login with local callback listener
schwab-agent auth login-url       # get OAuth URL
schwab-agent auth exchange --redirect-url "https://..."
schwab-agent auth refresh         # refresh expired token
schwab-agent auth status          # check token state
```

`auth login` keeps the local HTTPS callback listener alive through browser certificate-warning probes and other incomplete requests. It exits only after receiving a complete Schwab OAuth callback, an OAuth error callback, a state mismatch, or the configured timeout.

### market

Market data: `quote`, `history`.

```bash
schwab-agent market quote AAPL MSFT
schwab-agent market quote AAPL MSFT --fields sym,last,pct,vol
schwab-agent market quote AAPL MSFT --all-fields
schwab-agent market history SPY --period 10 --period-type day
schwab-agent market history SPY --fields ts,close,vol
schwab-agent market history SPY --all-fields
```

`market quote` is token-optimized by default. It returns `columns`, `rows`, and `rowCount` with compact default columns: `req`, `sym`, `bid`, `ask`, `last`, `mark`, `chg`, `pct`, `vol`, `err`. The `req` and `err` columns make per-symbol Schwab quote errors visible without expanding to the full detailed output. Use `--fields` to select output columns. Accepted aliases include full names such as `requested_symbol`, `symbol`, `net_change`, `net_percent_change`, `volume`, and `error`, plus compact names such as `req`, `sym`, `chg`, `pct`, `vol`, and `err`. Use `--all-fields` to return the full detailed quote objects. Use `--api-fields quote,reference` only when you need to limit Schwab quote field groups requested from the API.

`market history` is also token-optimized by default. It returns `symbol`, `columns`, `rows`, and `rowCount` with compact default candle columns: `ts`, `open`, `high`, `low`, `close`, `vol`. Use `--fields` to select candle columns. Accepted aliases include full names such as `timestamp`, `datetime`, `datetimeISO8601`, and `volume`, plus compact names such as `ts`, `iso`, `o`, `h`, `l`, `c`, and `vol`. Use `--all-fields` to return the full Schwab price history object, including fields such as `previousClose`, `previousCloseDate`, `previousCloseDateISO8601`, `empty`, and raw candle objects.

### account

Account discovery, balances, positions, and resolution for LLM agents.

Use `account` without a selector to list available account hashes and nicknames, then pass the chosen value to `--account` in order commands. Pass an account hash or nickname as the optional selector to resolve it to the canonical hash. Add `--positions` with a selector when you want the selected account summary plus holdings instead of hash resolution.

```bash
schwab-agent account                                    # list all accounts with balances
schwab-agent account --positions                        # include holdings as compact objects
schwab-agent account Trading                            # resolve nickname to canonical hash
schwab-agent account Trading --positions                # selected account summary with holdings
```

Position output with `--positions` returns compact position objects with all curated fields Schwab provides: `symbol`, `cusip`, `instrument_id`, `description`, `asset_type`, `long_quantity`, `short_quantity`, `average_price`, `market_value`, `current_day_profit_loss`, and `current_day_profit_loss_percentage`. Missing Schwab fields are omitted from each position object; `cusip` and `instrument_id` are included when available so positions without symbols still have actionable instrument identifiers.


### order

Unified order workflow for equity and option placement, lifecycle management, and raw JSON submission.

**Equity actions** (`order equity`): `buy`, `sell`, `sell-short`, `buy-to-cover`. Each hardcodes the Schwab `Instruction` to prevent accidental trade reversal.

**Option actions** (`order option`): `buy-to-open`, `sell-to-open`, `buy-to-close`, `sell-to-close`. Requires a full OCC symbol (e.g., `AAPL  250117C00150000`). Each hardcodes the Schwab `Instruction`. For multi-leg orders, use `order place-raw`.

The `-a`/`--account` flag controls execution mode: omit for dry-run (prints order JSON locally), pass `--account` to place directly, add `--save-preview` to preview and save a digest, or add `--preview-first` to preview then place automatically.

Lifecycle subcommands: `get`, `cancel`, `replace`, `repeat`. `order get` without arguments returns active orders across all linked accounts. Pass `--account HASH_OR_NICKNAME` to return active orders for one account, `--symbol SYMBOL` to keep only orders whose legs include that instrument symbol, `--include-inactive` to keep inactive orders, or `--account HASH_OR_NICKNAME --order ORDER_ID` to fetch one specific order. `replace` requires `--account` and `--order-id`, then an `equity` or `option` subcommand with the new payload. `repeat` fetches an existing order, rebuilds a new order payload from the supported historical fields, and can place directly, save a preview digest, or preview first.

```bash
# Equity orders
schwab-agent order equity buy AAPL -q 10 --price 150.00                          # dry-run
schwab-agent order equity buy AAPL -q 10 --price 150.00 -a HASH --save-preview   # preview + save digest
schwab-agent order place-from-preview -a HASH_OR_NICKNAME -d DIGEST_HEX          # place from saved preview
schwab-agent order equity sell AAPL -q 10 --price 155.00 -a HASH                 # direct place

# Option orders (OCC symbol required)
schwab-agent order option buy-to-open "AAPL  250117C00150000" -q 1 --price 5.00
schwab-agent order option buy-to-open "AAPL  250117C00150000" -q 1 --price 5.00 -a HASH --save-preview
schwab-agent order place-from-preview -a HASH -d DIGEST_HEX

# Lifecycle
schwab-agent order get
schwab-agent order get --account HASH_OR_NICKNAME
schwab-agent order get --symbol IBM
schwab-agent order get --include-inactive --from 2025-01-01 --to 2025-01-31
schwab-agent order get --account HASH_OR_NICKNAME --order 12345678
schwab-agent order replace -a HASH --order-id 12345678 equity buy AAPL -q 10 --price 148.00
schwab-agent order repeat -a HASH_OR_NICKNAME --order-id 12345678 --save-preview
schwab-agent order cancel --account HASH --order-id 12345678
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

Returns quote + TA dashboard for each symbol. Partial failures include per-symbol error fields (`quote_error`, `analysis_error`) alongside successful results. The default `--points 1` returns only the latest indicator values, which is sufficient for agent decision-making and reduces token usage by ~88% compared to the historical 20-point default. Use `--points N` when you need a time series for trend analysis.

## Order Workflow

The recommended agent workflow uses tamper-evident previews:

1. Pass `--account HASH --save-preview` to preview the order and save a digest to disk.
2. `order place-from-preview --account HASH --digest DIGEST` submits the exact saved payload after SHA-256 digest, 15-minute TTL, and account checks pass.

Previews are stored in `$XDG_STATE_DIR/schwab-agent/previews/`.

### Post-Action Verification

All mutable order actions (place, place-from-preview, place-raw, replace, repeat, cancel) automatically follow up with a GET to retrieve the order status. Schwab's API only returns a Location header and order ID on placement and replacement, so the CLI verifies by fetching the full order. The response preserves the existing `order_id`, `location`, and submitted `order` fields, and adds `verification_state`, optional `verification_failures`, and `verified_order` when the follow-up GET returns order details.

`order get` defaults to cross-account active-order discovery. Active orders are returned orders whose `status` exactly matches one of the strings in the `active_statuses` output field. Any other returned status is treated as inactive and is included only with `--include-inactive`. Add `--symbol SYMBOL` to keep only orders whose `orderLegCollection` includes a matching instrument symbol; matching is case-insensitive, multi-leg orders are included when any leg matches, and no matches returns a successful empty `orders` array. The command fetches raw Schwab order JSON before sanitizing output so newer order activity values such as canceled executions do not break discovery. If Schwab returns an unrecognized activity enum value, the response still includes the order and adds a sanitized `warnings` array with the field, value, and count.

`order get --from` and `--to` accept either date-only values (`YYYY-MM-DD`) or exact RFC3339 instants. Date-only ranges are interpreted as inclusive UTC calendar days, so `--from 2026-05-28 --to 2026-05-31` searches from `2026-05-28T00:00:00Z` through `2026-05-31T23:59:59.999999999Z`. Date filters, `--recent`, `--symbol`, and `--include-inactive` are discovery-mode filters and cannot be combined with `--order`.

`order cancel` accepts the order ID either positionally (`order cancel --account HASH 12345678`) or as `--order-id 12345678`.

`order repeat` accepts the order ID either positionally (`order repeat --account HASH 12345678`) or as `--order-id 12345678`. It supports Schwab order conversion for `SINGLE`, `TRIGGER`, and `OCO` orders with equity or option legs. Response-only metadata such as the original order ID, status, timestamps, account number, and fill history is dropped before the new payload is submitted or saved. Unsupported historical shapes return `order.validation_failed`; use `order place-raw` when you need to manually adapt a complex order Schwab cannot convert.

## Testing

```bash
make check    # fmt + clippy + test + doc (runs both default and decimal feature configs)
make test     # tests only (default + decimal)
make coverage # cargo llvm-cov, 90% line coverage threshold
make patch-coverage # lcov + diff-cover, 100% changed-line threshold against main
make audit    # cargo audit
```

CI runs on Ubuntu, macOS, and Windows with MSRV verification against 1.95. The coverage job uploads `lcov.info` to Codecov. Codecov requires 90% project coverage with a 2% tolerance for the current baseline and 100% patch coverage for changed lines. Run `make patch-coverage` before opening a PR; override the comparison branch with `PATCH_COVERAGE_BASE=<branch>` or use `DIFF_COVER='uvx diff-cover'` if `diff-cover` is not installed as a standalone command.

## Release

Releases are fully automated on push to main using three chained components:

1. **git-cliff** generates changelogs from Conventional Commits
2. **release-plz** creates/updates a release PR, publishes to crates.io via Trusted Publishing, and creates git tags
3. **cargo-dist** builds cross-platform binaries with Rust 1.95 (x86_64/aarch64 Linux, x86_64/aarch64 macOS, x86_64 Windows) and creates GitHub Releases with shell and PowerShell installers

Push commits to `main`, release-plz opens a release PR. Merge it, and the pipeline publishes the crate, tags the release, builds binaries, and creates the GitHub Release automatically.

The first crate release must be published manually with a crates.io token that has `publish-new` scope. After that first publish, configure crates.io Trusted Publishing for this repository with workflow filename `cd.yml`; subsequent publishes use GitHub Actions OIDC instead of a `CARGO_REGISTRY_TOKEN` secret.

## License

MIT
