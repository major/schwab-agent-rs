# schwab-agent-rs

Agent-oriented JSON CLI porcelain for the Charles Schwab API, built on top of [schwab-rs](https://github.com/major/schwab-rs).

> **Disclaimer:** This project is unofficial and is not affiliated with, endorsed by, or connected to Charles Schwab, TD Ameritrade, or thinkorswim in any way. Use at your own risk.

<!-- Uncomment once a GitHub remote is configured:
[![CI](https://github.com/major/schwab-agent-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/major/schwab-agent-rs/actions/workflows/ci.yml)
-->

| | |
|---|---|
| License | MIT |
| MSRV | 1.88 |
| Edition | 2024 |

## Overview

`schwab-agent` is a CLI binary that wraps the `schwab` crate and emits structured JSON for every command. It is designed for LLM agents and automation pipelines that need predictable, machine-readable output from the Schwab brokerage API.

All output uses a versioned `Envelope<T>` JSON wrapper:

```json
{
  "ok": true,
  "command": "market.quote",
  "schema_version": 1,
  "data": { "..." : "..." },
  "warnings": [],
  "meta": { "..." : "..." }
}
```

Errors use the same envelope shape with an `error` field instead of `data`.

## Prerequisites

- Rust toolchain (stable, >= 1.88)
- The [`schwab-rs`](https://github.com/major/schwab-rs) crate checked out as a sibling directory (`../schwab-rs`)
- A Charles Schwab developer application (client ID + secret)

## Building

```bash
cargo build --release
```

The `decimal` feature flag switches price types to fixed-point decimals:

```bash
cargo build --release --features decimal
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

### portfolio

Account snapshot with optional positions.

```bash
schwab-agent portfolio snapshot --account HASH --positions
```

### stock

Equity order workflow with four actions: `buy`, `sell`, `sell-short`, `buy-to-cover`.

Subcommands: `build`, `preview`, `place`, `place-from-preview`, `preview-raw`, `place-raw`.

Each action hardcodes the Schwab `Instruction` to prevent accidental trade reversal.

```bash
schwab-agent stock build buy AAPL --quantity 10 --price 150.00
schwab-agent stock preview buy AAPL --quantity 10 --price 150.00 --account HASH --save-preview
schwab-agent stock place-from-preview --preview-file /path/to/preview.json
```

### order

Option order workflow supporting 15 named strategies: `long-call`, `long-put`, `cash-secured-put`, `naked-call`, `sell-covered-call`, `bull-call-spread`, `bear-call-spread`, `bull-put-spread`, `bear-put-spread`, `long-straddle`, `short-straddle`, `long-strangle`, `short-strangle`, `short-iron-condor`, `jade-lizard`.

Subcommands: `build`, `preview`, `place`, `place-from-preview`.

Each strategy hardcodes contract type and direction to prevent accidental trade reversal.

### option

Option chain data: `expirations`, `chain`, `screen`, `contract`.

```bash
schwab-agent option expirations AAPL
schwab-agent option chain AAPL --expiration 2025-06-20 --type CALL
schwab-agent option screen AAPL --expiration 2025-06-20 --min-delta 0.20 --max-delta 0.40
schwab-agent option contract AAPL250620C00200000
```

## Order Workflow

The recommended agent workflow uses tamper-evident previews:

1. `preview --save-preview` - preview the order and save to disk
2. `place-from-preview` - submit the exact saved payload after SHA-256 digest, 15-minute TTL, and account checks pass

Direct `place` is available for explicit human use, but agents should prefer the preview workflow.

Previews are stored in `$XDG_STATE_DIR/schwab-agent/previews/`.

## Testing

```bash
make check    # fmt + clippy + test + doc (runs both default and decimal feature configs)
make test     # tests only (default + decimal)
make coverage # cargo llvm-cov, 90% line coverage threshold
make audit    # cargo audit
```

CI runs on Ubuntu, macOS, and Windows with MSRV verification against 1.88.

## License

MIT
