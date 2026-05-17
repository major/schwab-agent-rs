---
applyTo: ".github/workflows/release-plz.yml"
---

# Release review instructions

- Release automation uses `release-plz` for changelog generation, crate publishing, git tags, and GitHub releases.
- This repository publishes the Rust CLI crate to crates.io. Do not add binary artifact build matrices unless the release strategy explicitly changes.
- The workflow is manual-only. Do not add push triggers unless the project intentionally moves back to automatic release runs.
- The release PR job must use `RELEASE_PLZ_TOKEN` so release PR branch pushes trigger normal CI workflows.
