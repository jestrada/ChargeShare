# Proposal

## Why

The merged dashboard prices sample sessions with fictional rates. We need to
try reviewed historical electricity rates without changing charging evidence,
and record when each rate version applies so later updates do not silently
change the meaning of earlier dates.

## What Changes

- Add an optional local rate file with explicit effective dates, stable version
  IDs, source labels and daily time-of-use windows. Rates are exact combined
  variable USD/kWh amounts supplied by the operator.
- Select the applicable version for each sample date and reuse the existing Rust
  pricing engine. Missing coverage and unresolved boundaries remain visible holds.
- Allow the loopback preview to load this file explicitly at startup. The normal
  command continues to run the existing fictional demo without private files.
- Identify locally supplied historical rates separately from fictional sessions,
  show source/version details and preserve useful rate precision in the UI.
- Keep real bills, derived household profiles and private verification outside
  the repository. Public examples and automated checks remain synthetic.

This PR starts with planning artifacts. Implementation may follow in the same PR
after explicit approval of this scope in the review conversation. All build tasks
remain unchecked until implemented and verified.

### Non-goals

No Tesla connection, live session ingestion, current-price claim, automatic tariff
download, bill parser, utility component calculator, invoice or payment collection.
No allocation of fixed charges, credits or charging losses. No hosting, database,
authentication, new settings page or change to the agreed backend responsibilities.
General timezone/DST conversion and real billing-cycle accounting remain separate
work: this increment maps rates onto the existing frozen September preview only.

## Capabilities

### New Capabilities

- `dated-rate-profiles`: validated date-bounded rate versions, explicit local
  loading and transparent repricing of the synthetic preview.

### Modified Capabilities

None in the canonical spec inventory. `vehicle-ledger` remains unchanged.
The implemented `session-pricing` and `local-dashboard` contracts currently live
in completed, unarchived changes. This proposal extends their optional preview
configuration; their fictional default and conservative pricing rules remain.
Approval of this change explicitly permits local rate-only input in that mode,
where the earlier dashboard contract otherwise required no private file access.

## Impact

Planned implementation is concentrated in `crates/chargeshare-preview`, the
preview launcher, dashboard types/rendering and local-preview documentation.
Existing `RateSchedule`, `UsdRate` and vehicle-scoped ledger queries are reused;
event timestamps retain their existing synthetic meaning. The core needs no IO,
calendar dependency or public API change.

The local response will gain rate-source metadata and explicit unavailable rate
outlook values. Frontend and API changes ship together. Startup configuration
adds an optional flag, with no new HTTP route. No infrastructure or account setup
is needed to review this proposal or exercise its eventual synthetic tests.
