# AGENTS.md - schwab-agent-rs

> **KEEP DOCS IN SYNC.** Any change to commands, args, output format, error codes, features, or workflows MUST be reflected in both `AGENTS.md` and `README.md` as part of the same PR. Stale docs are worse than no docs.

## What This Is

Rust CLI binary (`schwab-agent`) wrapping the `schwab` crate to provide agent-oriented structured JSON output for Charles Schwab API workflows. Not a library - it's a CLI porcelain. The `schwab` crate is resolved from crates.io for CI compatibility.

- Edition 2024, MSRV 1.95
- `publish = false` (private crate)
- Feature flag: `decimal` (enables `schwab/decimal`) - enabled by default

## Source Layout

```text
src/
  main.rs          - Entry point, delegates to lib.rs::run_from_env()
  lib.rs           - Orchestrator: CLI parsing, command dispatch, JSON envelope output
  cli.rs           - clap derive CLI definition with subcommands and global args
  output.rs        - Envelope<T> (versioned JSON wrapper: ok, command, data/error, warnings, meta)
  shared.rs        - Shared types: SessionChoice, DurationChoice, to_number() helper
  config.rs        - Agent config: load shared config, mutable-operation guard
  error/
    mod.rs         - AppError enum (thiserror) with stable codes, exit codes, categories, hints
    tests.rs       - Error module tests
  auth/
    mod.rs         - Auth commands: status, login, login-url, exchange, refresh
    tests.rs       - Auth module tests
  equity/
    mod.rs         - Stock order commands: buy, sell, sell-short, buy-to-cover + raw JSON
    tests.rs       - Equity module tests
  market/
    mod.rs         - Market commands: history, quote. opt_field! macro, summarize_quote()
    tests.rs       - Market module tests
  verify.rs        - Post-action verification: OrderActionResult, verify_order(), action_envelope()
  order/
    mod.rs         - Option order dispatch, 15 named option strategies, inline tests
    builder.rs     - OCC symbol construction (21-char format), inline tests
    preview.rs     - SHA-256 tamper-evident preview with 15-min TTL (shared by equity + order)
    lifecycle.rs   - Order lifecycle commands: list, get, cancel with post-action verification
  portfolio/
    mod.rs         - Portfolio snapshot with optional positions
    tests.rs       - Portfolio module tests
  account/
    mod.rs         - Account commands: summary, resolve; account resolver; balance renderer
    tests.rs       - Account module tests
  options/
    mod.rs         - Module root: re-exports subcommand modules
    types.rs       - Shared types: FieldDef, FlatContract, flatten_chain, sort_contracts, select_fields, validate_fields, compute_dte, filter predicates
    tests.rs       - Options module tests
    expirations.rs - Expirations command: list available expiration dates
    chain.rs       - Chain command: option chain with server+client filtering
    contract.rs    - Contract lookup: single contract with curated flat output
    screen.rs      - Screen command: chain screening with liquidity/pricing filters
  ta/
    mod.rs           - Module root: re-exports dashboard handler
    types.rs         - Output types: DashboardOutput, ExpectedMoveOutput, AnalyzeOutput, signal types
    indicators.rs    - 8 hand-rolled TA indicators: SMA, EMA, RSI, MACD, ATR, BBands, Stochastic, ADX
    custom.rs        - Custom indicators: VWAP, Historical Volatility
    interval.rs      - Interval enum, Schwab API parameter mapping
    candles.rs       - Candle data extraction and validation helpers
    dashboard.rs     - Dashboard command handler with category-grouped output
    expected_move.rs - Expected move from ATM option straddle pricing
    tests.rs         - TA module tests
  analyze/
    mod.rs           - Multi-symbol analyze command with partial-failure support
    tests.rs         - Analyze module tests
```

## Command Groups

- **auth** - Token management (status, login, login-url, exchange, refresh)
- **market** - Market data (history, quote)
- **account** - Account discovery and resolution (summary, resolve)
- **stock** - Equity order workflow (build, preview, place, place-from-preview, preview-raw, place-raw)
- **order** - Option order workflow (build, preview, place, replace, place-from-preview) + lifecycle (list, get, cancel)
- **portfolio** - Account snapshot with optional positions
- **option** - Option chain data (expirations, chain, screen, contract)
- **ta** - Technical analysis (dashboard, expected-move)
- **analyze** - Multi-symbol analysis with partial-failure support

### Stock Actions (4 total)

buy, sell, sell-short, buy-to-cover

Each action hardcodes the Schwab `Instruction` to prevent accidental trade reversal. Supports order types: market (default), limit, stop, stop-limit.

`preview-raw` and `place-raw` accept arbitrary JSON payloads for complex order types (bracket, OCO, triggered orders) that use recursive `childOrderStrategies`.

### Option Strategies (15 total)

long-call, long-put, cash-secured-put, naked-call, sell-covered-call, call-debit-spread, call-credit-spread, put-debit-spread, put-credit-spread, long-straddle, short-straddle, long-strangle, short-strangle, short-iron-condor, jade-lizard

Each strategy hardcodes contract type and direction to prevent accidental trade reversal.

### Order Workflow

Recommended LLM workflow: `preview --save-preview` -> `place-from-preview`.

`build` is an optional local dry run. Direct `place` is available for explicit human requests, but LLM agents should use saved previews because `place-from-preview` submits the exact saved preview payload after the SHA-256 digest, 15-minute TTL, and account checks pass. Previews are stored in `$XDG_STATE_DIR/schwab-agent/previews/`.

Agents should prefer limit-style pricing whenever practical: pass `--price` so single-leg orders use `LIMIT` and multi-leg orders use `NET_DEBIT` or `NET_CREDIT`. Omitting `--price` intentionally creates a market order and should be reserved for cases where market execution is explicitly desired.

### Mutable Operation Guard

All mutable commands (place, place-from-preview, place-raw, replace, cancel) check `~/.config/schwab-agent/config.json` for `"i-also-like-to-live-dangerously": true` before executing. The config file is shared with the Go CLI.

- Missing config file or missing key = mutable operations disabled (safe default)
- Guard function: `config::require_mutable_enabled()` returns `AppError::MutableDisabled` (exit code 10, error code `config.mutable_disabled`)
- Guard is called inside the order/equity dispatch handlers, before any API call
- Read-only commands (build, preview, list, get) are NOT gated

### Post-Action Verification

All mutable order actions (place, place-from-preview, place-raw, replace, cancel) immediately follow up with a GET to retrieve the order status. This is critical for LLM agents because Schwab's place/replace response only returns a Location header and order ID, not the actual order state.

The verification module (`src/verify.rs`) provides:

- `OrderActionResult` struct with the existing `order_id`, `location`, and submitted `order` fields, plus `verification_state` ("verified" or "unverified"), optional `verification_failures`, and the follow-up GET payload in `verified_order`
- `verify_order()` does a best-effort GET after any mutable action; on failure it returns `unverified` with failure details instead of propagating the error
- `action_envelope()` builds the Envelope with warnings extracted from unverified failures

### Order Lifecycle Commands

`order list`, `order get`, `order replace`, and `order cancel` manage existing orders.

- **list**: All orders or per-account, with status filtering, date range, and `--recent` (24h lookback). Defaults to 60 days if no `--from` specified. `--from` and `--to` accept `YYYY-MM-DD` or RFC3339; date-only values are inclusive UTC calendar days.
- **get**: Single order by ID (positional arg), requires `--account`.
- **replace**: Replace an existing option order by positive order ID, requires `--account`, then a safe strategy payload (e.g., `long-call ...`). Includes post-replace verification via GET.
- **cancel**: Cancel by order ID (positive positional arg), requires `--account`. Includes post-cancel verification via GET and only reports `verified` once the fetched status is `CANCELED`.

### Option Data Subcommands (4 total)

expirations, chain, screen, contract

Row-based output (columns + rows arrays) for expirations, chain, and screen. Flat object output for contract. All include underlying symbol context.

Recommended LLM workflow: `expirations` (pick date) -> `chain` (with filters) -> `contract` (for detail). Use `screen` for multi-criteria filtering with liquidity and pricing constraints.

## CLI Global Args

`--token`, `--client-id`, `--client-secret`, `--callback-url`

Token path env var: `SCHWAB_TOKEN_PATH`. Default: `$XDG_CONFIG_DIR/schwab-agent-rs/token.json`.

## Output Format

All output uses `Envelope<T>` - a versioned JSON wrapper:

```json
{
  "ok": true,
  "command": "market.quote",
  "schema_version": 1,
  "data": { ... },
  "warnings": [],
  "meta": { ... }
}
```

Errors use the same envelope with `ErrorBody` in the `error` field. Schema version constant: `SCHEMA_VERSION = 1`.

### Error Codes and Exit Codes

- 3 = auth errors
- 4 = HTTP status errors
- 10 = input/validation/config errors (includes ta.insufficient_data, ta.invalid_interval, config.mutable_disabled)
- 11 = preview errors
- 20 = IO/JSON/config errors (includes ta.calculation_error)

## Key Dependencies

- `clap` (derive) - CLI parsing
- `schwab` - Schwab API client
- `serde` / `serde_json` - Serialization
- `serde_with` - `skip_serializing_none` for clean JSON
- `thiserror` - Error derivation
- `tokio` - Async runtime
- `time` - Date/time handling
- `sha2` - Preview digest
- `tempfile` (dev) - Test fixtures

## Build and Test

Use `make check` for the full suite. Individual targets:

```bash
make fmt          # cargo fmt --all --check
make fmt-fix      # cargo fmt --all
make clippy       # Runs twice: default + --features decimal
                  # Flags: -D clippy::all -A clippy::needless_borrow -A clippy::large_enum_variant -A clippy::too_many_arguments
make test         # Runs twice: default + --features decimal
make doc          # Checks for broken intra-doc links
make coverage     # cargo llvm-cov test --fail-under-lines 90
make audit        # cargo audit
make check        # fmt + clippy + test + doc (aggregate)
```

Always run both default and `decimal` feature configurations. CI does the same.

## Conventions

### Code Style

- Every module uses `#[cfg(test)] mod tests;` - separate test files for auth, error, equity, market, portfolio; inline tests for lib, cli, output, builder, preview, order/mod, verify, lifecycle
- Docstrings on all public items and many private items
- `#[must_use]` on pure functions
- `serde_with::skip_serializing_none` for clean JSON output
- Tests use standard `assert!`/`assert_eq!` macros, `#[tokio::test]` for async tests
- Conventional commit messages

### Patterns to Follow

- New commands go in their command group module and get wired through `cli.rs` and `lib.rs`
- All command output wraps in `Envelope<T>` - never print raw data to stdout
- Errors use `AppError` variants with stable error codes, exit codes, categories, and hints
- Order strategies hardcode contract type + direction (safety invariant)
- Stock actions hardcode instruction (safety invariant)
- Order commands produce dynamic command names (e.g., `order.build.long-call`, `stock.build.buy`)
- Mutable order actions (place, replace, cancel) use `verify::verify_order()` for post-action verification
- Lifecycle commands (order list/get/replace/cancel) use static command names (e.g., `order.list`, `order.get`, `order.replace`, `order.cancel`)

### Testing

- nextest configured: default profile has 2 retries, 30s slow timeout, 1s leak timeout
- CI profile: 3 retries, no fail-fast
- Coverage threshold: 90% line coverage

## CI

### ci.yml

- fmt (nightly rustfmt), clippy (stable), test (stable), MSRV (1.95, `--locked`), docs (stable)
- Uses pinned action SHAs

### audit.yml

- `cargo audit` on push/PR when Cargo files change, plus daily cron

## Security

Keep account hashes, tokens, and credentials out of logs, errors, tests, and docs. The preview system uses cryptographic digests specifically to avoid storing sensitive order data in plaintext.

## Tooling Config

- **CodeRabbit** (`.coderabbit.yaml`): auto-review disabled (manual trigger via `@coderabbitai review`). References `**/AGENTS.md` as code guideline source.
- **Renovate**: weekly Monday dep updates, auto-merge patch/minor after 7 days.
- **nextest** (`.config/nextest.toml`): retry and timeout configuration.

## Files to Keep Updated

When the project changes (new commands, strategies, args, error codes, CI config, etc.), update:

- **`README.md`** - project overview and usage for GitHub
- **`AGENTS.md`** - this file
- **`SKILL.md`** - LLM-facing CLI usage guide
- **`.coderabbit.yaml`** - path instructions and review guidelines
