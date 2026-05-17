# AGENTS.md - schwab-agent-rs

> **KEEP DOCS IN SYNC.** Any change to commands, args, output format, error codes, features, or workflows MUST be reflected in both `AGENTS.md` and `README.md` as part of the same PR. Stale docs are worse than no docs.

## What This Is

Rust CLI binary (`schwab-agent`) wrapping the `schwab` crate to provide agent-oriented structured JSON output for Charles Schwab API workflows. Not a library - it's a CLI porcelain. The `schwab` crate is resolved from crates.io for CI compatibility.

- Edition 2024, MSRV 1.95
- Published crate once manually, then released through `release-plz` with crates.io Trusted Publishing
- Feature flag: `decimal` (enables `schwab/decimal`) - enabled by default

## Source Layout

```text
src/
  main.rs          - Entry point, delegates to lib.rs::run_from_env()
  lib.rs           - Orchestrator: CLI parsing, command dispatch, JSON output
  cli.rs           - clap derive CLI definition with subcommands and global args
  output.rs        - ErrorBody struct for structured error JSON output
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
    mod.rs         - Market commands: history, quote. opt_field! macro, summarize_quote(), compact quote/history rows
    tests.rs       - Market module tests
  verify.rs        - Post-action verification: OrderActionResult, verify_order(), action_value()
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
- `action_value()` serializes the `OrderActionResult` directly to `Value` (verification failures are already in the struct)

### Order Lifecycle Commands

`order list`, `order get`, `order replace`, and `order cancel` manage existing orders.

- **list**: All orders or per-account, with status filtering, date range, and `--recent` (24h lookback). Defaults to 60 days if no `--from` specified. `--from` and `--to` accept `YYYY-MM-DD` or RFC3339; date-only values are inclusive UTC calendar days.
- **get**: Single order by ID (positional arg), requires `--account`.
- **replace**: Replace an existing option order by positive order ID, requires `--account`, then a safe strategy payload (e.g., `long-call ...`). Includes post-replace verification via GET.
- **cancel**: Cancel by order ID (positive positional arg), requires `--account`. Includes post-cancel verification via GET and only reports `verified` once the fetched status is `CANCELED`.

### Option Data Subcommands (4 total)

expirations, chain, screen, contract

Row-based output (columns + rows arrays) for expirations, chain, and screen. Flat object output for contract. All include underlying symbol context.

### Market Quote and History Output

`market quote` is token-optimized by default and returns row-based output with `columns`, `rows`, and `rowCount`. Default columns are `req`, `sym`, `bid`, `ask`, `last`, `mark`, `chg`, `pct`, `vol`, and `err` so per-symbol quote errors stay visible in compact output. Use `--fields` to select output columns by compact names or full aliases such as `requested_symbol`, `symbol`, `net_change`, `net_percent_change`, `volume`, and `error`. Use `--all-fields` to return full detailed quote objects. Use `--api-fields quote,reference` to limit Schwab quote field groups requested from the API.

`market history` is token-optimized by default and returns row-based output with `symbol`, `columns`, `rows`, and `rowCount`. Default columns are `ts`, `open`, `high`, `low`, `close`, and `vol`. Use `--fields` to select candle columns by compact names or aliases such as `timestamp`, `datetime`, `datetimeISO8601`, `iso`, `o`, `h`, `l`, `c`, and `volume`. Use `--all-fields` to return the full Schwab price history object, including previous-close metadata and raw candle objects.

Recommended LLM workflow: `expirations` (pick date) -> `chain` (with filters) -> `contract` (for detail). Use `screen` for multi-criteria filtering with liquidity and pricing constraints.

## CLI Global Args

`--token`, `--client-id`, `--client-secret`, `--callback-url`

Token path env var: `SCHWAB_TOKEN_PATH`. Default: `$XDG_CONFIG_DIR/schwab-agent-rs/token.json`.

## Output Format

Commands output raw JSON data payloads directly (no wrapper). Errors output an `ErrorBody` JSON object with `code`, `message`, `category`, `retryable`, and `hint` fields.

### Error Codes and Exit Codes

- 3 = auth errors
- 4 = HTTP status errors
- 10 = input/validation/config errors (includes market.validation_failed, ta.insufficient_data, ta.invalid_interval, config.mutable_disabled)
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
- All command output is raw JSON data payloads; errors use `ErrorBody` struct
- Errors use `AppError` variants with stable error codes, exit codes, categories, and hints
- Order strategies hardcode contract type + direction (safety invariant)
- Stock actions hardcode instruction (safety invariant)
- Mutable order actions (place, replace, cancel) use `verify::verify_order()` for post-action verification

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

### release-plz.yml

Release automation follows the same strategy as `schwab-rs`: `release-plz` runs on manual `workflow_dispatch` only. Two independent jobs are defined:

- `release-pr`: opens or updates a release PR with the version bump, `Cargo.lock` update, and `CHANGELOG.md` entries from Conventional Commits
- `release`: publishes to crates.io, creates the git tag, and creates the GitHub release after a version bump lands on `main`

The workflow uses crates.io Trusted Publishing with GitHub Actions OIDC (`id-token: write`) instead of `CARGO_REGISTRY_TOKEN`. Configure the crates.io Trusted Publisher for workflow filename `release-plz.yml` after the first manual crate publish succeeds. crates.io requires the first release of a brand-new crate to be published manually with a token that has `publish-new` scope before Trusted Publishing can be enabled.

The `release-pr` job uses the `RELEASE_PLZ_TOKEN` repository secret instead of the default `GITHUB_TOKEN` so release PR branch pushes trigger normal CI workflows. The `release` job can keep using `GITHUB_TOKEN` because publishing is authorized by crates.io Trusted Publishing and the job does not need to trigger another workflow.

Configuration lives in `release-plz.toml` and enables semver checking, changelog updates, git tags, and GitHub releases.

#### Release Workflow

Manual trigger flow (Actions > Release-plz > Run workflow):

1. Push commits to `main` using Conventional Commits (`feat:`, `fix:`, etc.)
2. When ready to release, trigger the workflow manually from GitHub Actions
3. `release-pr` opens a PR with the version bump, `Cargo.lock` update, and `CHANGELOG.md` entries
4. Review and merge the release PR
5. Trigger the workflow again to publish
6. `release` detects the version bump, runs `cargo publish`, creates the git tag, and creates the GitHub release
7. Verify at `https://crates.io/crates/schwab-agent-rs`

#### First Manual Publish

Trusted Publishing cannot be configured for a brand-new crate until the crate exists on crates.io. For the first release only:

1. Ensure `cargo publish --dry-run` succeeds locally
2. Publish manually with a crates.io token that has `publish-new` scope
3. Configure crates.io Trusted Publishing for this repository and workflow filename `release-plz.yml`
4. Use the manual `release-plz` workflow for later releases

#### Manual Release Fallback

If `release-pr` is unavailable, version bumps can be done manually:

1. Bump `version` in `Cargo.toml`
2. Run `cargo update --workspace` to sync `Cargo.lock`
3. Commit both `Cargo.toml` and `Cargo.lock` together (dirty `Cargo.lock` causes `cargo publish` to fail)
4. Push to `main`
5. Trigger the release-plz workflow manually, or run `cargo publish` locally

## Security

Keep account hashes, tokens, and credentials out of logs, errors, tests, and docs. The preview system uses cryptographic digests specifically to avoid storing sensitive order data in plaintext.

## Tooling Config

- **CodeRabbit** (`.coderabbit.yaml`): auto-review disabled (manual trigger via `@coderabbitai review`). References `**/AGENTS.md` as code guideline source.
- **Renovate**: weekly Monday dep updates, auto-merge patch/minor after 7 days.
- **nextest** (`.config/nextest.toml`): retry and timeout configuration.
- **release-plz** (`release-plz.toml`, `.github/workflows/release-plz.yml`): manual-only release PR and publish workflow with crates.io Trusted Publishing.

## Files to Keep Updated

When the project changes (new commands, strategies, args, error codes, CI config, etc.), update:

- **`README.md`** - project overview and usage for GitHub
- **`AGENTS.md`** - this file
- **`SKILL.md`** - LLM-facing CLI usage guide
- **`release-plz.toml`** - release-plz configuration (semver check, changelog, git tags, GitHub releases)
- **`.github/workflows/release-plz.yml`** - manual release PR and publish workflow
- **`.github/instructions/*.instructions.md`** - review instructions for workflow-specific policies
- **`.coderabbit.yaml`** - path instructions and review guidelines
