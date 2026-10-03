# Dated sample rates

## Why

Rates need effective dates so they can change without changing charging evidence.
The user approved the rate table and dashboard improvements together in PR #3.

## What Changes

- Commit a small dated rate table and select only the version covering a date.
- Replace fictional prices with a September public sample tariff and an October winter-estimate sample.
- Display precise rates and each priced session's version; leave coverage gaps held.
- Put Joseph’s sample reimbursement first, compact mobile batteries, show sample freshness/added energy and inline TOU splits, default to Joseph, and move scenarios into Settings.
- Derive sample period labels from the fixture clock and period boundaries.
- Archive the two merged pricing/dashboard changes and synchronize their specs.

## Capabilities

### Modified Capabilities
- `session-pricing`: dated versions independent of charging evidence.
- `local-dashboard`: sample tariff, version provenance and derived period labels.

## Impact

Changes the Rust preview adapter and dashboard layout, context and controls. The
core pricing engine, event contract and dependencies stay unchanged. No private files,
startup flag, real timestamps, connector, hosting, authentication or account setup.
