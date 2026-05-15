# Copilot review guidance

- This CLI prioritizes trading safety over avoiding small read-only API calls. Do not suggest skipping account resolution or caching account metadata unless the code repeats resolution within a single command path and the change preserves canonical account hash validation plus nickname support.
- For order placement and verification paths, account selectors must resolve to canonical Schwab account hashes before mutable API calls.
- Position output intended for agent decisions must include instrument identifiers such as symbol and asset type. Quantities without identifiers are not actionable.
- Do not flag async test attributes unless the test body has no `.await` and no async-only setup.
- Tests that mutate process-global environment variables must restore them with a panic-safe guard.
