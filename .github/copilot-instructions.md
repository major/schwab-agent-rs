# Copilot review guidance

- This CLI prioritizes trading safety over avoiding small read-only API calls. Do not suggest skipping account resolution or caching account metadata unless the code repeats resolution within a single command path and the change preserves canonical account hash validation plus nickname support.
- For order placement and verification paths, account selectors must resolve to canonical Schwab account hashes before mutable API calls.
- Position output intended for agent decisions must include instrument identifiers such as symbol and asset type. Quantities without identifiers are not actionable.
- Do not flag async test attributes unless the test body has no `.await` and no async-only setup.
- Tests that mutate process-global environment variables must restore them with a panic-safe guard.
- `market quote --all-fields` is the escape hatch for the legacy detailed `symbols` plus `quotes` output shape. Do not suggest compact-row normalization in that path.
- Default compact `market quote` rows must keep per-request quote errors visible, including API-provided `invalid_symbols`, `invalid_cusips`, and `invalid_ssids` details when Schwab returns a generic `errors` quote row.
- Validate local quote output field selection before authentication or Schwab API calls when possible. Invalid `--fields` input should fail deterministically with `market.validation_failed`.
