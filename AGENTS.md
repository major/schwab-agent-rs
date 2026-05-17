# AGENTS.md - schwab-agent-rs

> **KEEP DOCS IN SYNC.** Any change to commands, args, output format, error codes, features, or workflows MUST be reflected in both `AGENTS.md` and `README.md` as part of the same PR. Stale docs are worse than no docs.

## What This Is

Rust CLI binary (`schwab-agent`) wrapping the `schwab` crate to provide agent-oriented structured JSON output for Charles Schwab API workflows. Not a library - it's a CLI porcelain. The `schwab` crate is resolved from crates.io for CI compatibility.

### Architecture Boundary: schwab-rs vs schwab-agent-rs

`schwab-rs` is a low-level API crate with nearly zero data processing. It handles auth, HTTP transport, and typed deserialization of Schwab API responses, but does NOT sanitize, transform, or work around API quirks. All data munging, response normalization, and workaround logic belongs in `schwab-agent-rs`. When the Schwab API returns unexpected formats (e.g., object-wrapped arrays, boolean `false` in numeric fields), the fix goes here in schwab-agent-rs using `Provider::token()` for auth and direct HTTP requests via reqwest, not in schwab-rs itself. The `raw` module (`src/raw.rs`) centralizes these workarounds.

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
  raw.rs           - Raw Schwab API requests with response normalization (object unwrap, false→null)
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
- **account** - Account discovery, balances, positions, and resolution (summary, resolve)
- **stock** - Equity order workflow (build, preview, place, place-from-preview, preview-raw, place-raw)
- **order** - Option order workflow (build, preview, place, replace, place-from-preview) + lifecycle (list, get, cancel)
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
- `reqwest` - Direct HTTP requests for raw API workarounds
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

- Every module uses `#[cfg(test)] mod tests;` - separate test files for auth, error, equity, market, account; inline tests for lib, cli, output, builder, preview, order/mod, verify, lifecycle, raw
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

### cd.yml (Continuous Deployment)

Release automation uses three chained components triggered by git events:

1. **git-cliff** (`cliff.toml` for CLI, `[changelog]` in `release-plz.toml` for CI) - Generates changelogs from Conventional Commits with emoji-prefixed groups (Features, Bug Fixes, Documentation, etc.). Skips `chore: release`, `chore(deps)`, `chore(pr)`, `chore(pull)` commits. The `[changelog]` section in `release-plz.toml` is the authoritative config for release PRs; `cliff.toml` is for standalone `git-cliff` CLI use only.
2. **release-plz** (`cd.yml` + `release-plz.toml`) - Runs on push to main. A single job runs `release-plz/action` without a specific command, so it handles both release-pr and release in one run. Creates/updates a release PR, and when a version bump lands on main, publishes to crates.io and creates a git tag. Uses crates.io Trusted Publishing (OIDC with `id-token: write`), no `CARGO_REGISTRY_TOKEN`. `git_release_enable = false` because cargo-dist creates GitHub Releases.
3. **cargo-dist** (`release.yml` + `dist-workspace.toml`) - Triggered by tag push matching `**[0-9]+.[0-9]+.[0-9]+*`. Builds cross-platform binaries for x86_64 Linux, x86_64/aarch64 macOS, x86_64 Windows. Generates shell and PowerShell installers. Creates the GitHub Release with all artifacts.

The `release-plz` job uses `RELEASE_PLZ_TOKEN` so release PR branch pushes trigger normal CI workflows.

`release.yml` is auto-generated by cargo-dist. Do not edit manually. Run `dist generate --ci github` to regenerate after changing `dist-workspace.toml`.

#### Release Workflow

Automatic flow on push to main:

1. Push commits to `main` using Conventional Commits (`feat:`, `fix:`, etc.)
2. `cd.yml` runs automatically, release-plz creates/updates a release PR with the version bump, `Cargo.lock` update, and `CHANGELOG.md` entries
3. Review and merge the release PR
4. Merge triggers `cd.yml` again, release-plz detects the version bump, publishes to crates.io, and creates a git tag
5. Git tag push triggers `release.yml`, cargo-dist builds binaries and creates the GitHub Release
6. Verify at `https://crates.io/crates/schwab-agent-rs`

#### First Manual Publish

Trusted Publishing cannot be configured for a brand-new crate until the crate exists on crates.io. For the first release only:

1. Ensure `cargo publish --dry-run` succeeds locally
2. Publish manually with a crates.io token that has `publish-new` scope
3. Configure crates.io Trusted Publishing for this repository with workflow filename `cd.yml`
4. Subsequent releases are fully automatic on push to main

#### Manual Release Fallback

If release-plz is unavailable, version bumps can be done manually:

1. Bump `version` in `Cargo.toml`
2. Run `cargo update --workspace` to sync `Cargo.lock`
3. Commit both `Cargo.toml` and `Cargo.lock` together (dirty `Cargo.lock` causes `cargo publish` to fail)
4. Push to `main`
5. release-plz picks up the version bump and publishes automatically

## Security

Keep account hashes, tokens, and credentials out of logs, errors, tests, and docs. The preview system uses cryptographic digests specifically to avoid storing sensitive order data in plaintext.

## Tooling Config

- **CodeRabbit** (`.coderabbit.yaml`): auto-review disabled (manual trigger via `@coderabbitai review`). References `**/AGENTS.md` as code guideline source.
- **Renovate**: weekly Monday dep updates, auto-merge patch/minor after 7 days.
- **nextest** (`.config/nextest.toml`): retry and timeout configuration.
- **git-cliff** (`cliff.toml` for CLI, `[changelog]` in `release-plz.toml` for CI): changelog generation from Conventional Commits with emoji-prefixed groups.
- **release-plz** (`release-plz.toml`, `.github/workflows/cd.yml`): push-to-main release PR and publish workflow with crates.io Trusted Publishing. Changelog config is inline in the `[changelog]` section (not via deprecated `changelog_config` file reference).
- **cargo-dist** (`dist-workspace.toml`, `.github/workflows/release.yml`): tag-triggered cross-platform binary builds and GitHub Releases. `release.yml` is auto-generated; run `dist generate --ci github` to regenerate.

## Files to Keep Updated

When the project changes (new commands, strategies, args, error codes, CI config, etc.), update:

- **`README.md`** - project overview and usage for GitHub
- **`AGENTS.md`** - this file
- **`SKILL.md`** - LLM-facing CLI usage guide
- **`cliff.toml`** - git-cliff changelog configuration
- **`release-plz.toml`** - release-plz configuration (semver check, changelog, git tags, crate publishing)
- **`dist-workspace.toml`** - cargo-dist configuration (targets, installers, CI)
- **`.github/workflows/cd.yml`** - push-to-main release-plz workflow
- **`.github/workflows/release.yml`** - auto-generated cargo-dist workflow (do not edit manually)
- **`.github/instructions/*.instructions.md`** - review instructions for workflow-specific policies
- **`.coderabbit.yaml`** - path instructions and review guidelines
