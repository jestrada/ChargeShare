# Offline session pricing

## Why

The implemented ledger separates eligible AC energy by vehicle but cannot explain
its cost. Add an offline pricing layer that preserves the ledger's uncertainty
instead of assigning an entire session the rate at its start.

## What Changes

- Add exact nonnegative USD-per-kWh rates, exact costs and explicit half-up cent rounding.
- Price the ledger's own counter segments against caller-supplied, versioned rate windows.
- Hold a whole session when a positive energy interval crosses a rate boundary,
  lacks a rate, or has no increasing time interval. Preserve existing exclusions.
- Return vehicle-scoped session quotes and an explicitly partial priced subtotal;
  keep rate versions, energy, interval bounds and evidence labels visible.
- Add a runnable fictional example and acceptance scenarios without runtime dependencies.

Implementation is authorized for this local feature branch. This document records
the bounded design before source edits; task checkboxes track actual completion.

## Capabilities

### New Capabilities

- `session-pricing`: Exact, conservative, vehicle-scoped pricing of offline AC evidence.

### Modified Capabilities

None. Existing Spec 1 public behavior and scenarios remain unchanged.

## Impact

Add cohesive money, tariff and pricing modules in `chargeshare-core`. Retain
counter segment evidence inside session reconstruction without changing the
existing public Session shape. Add a scoped Ledger pricing query, a synthetic
example, acceptance tests and documentation. No new dependencies.

This is not a calendar tariff engine or monthly invoice. Inputs use the existing
synthetic tick domain; the caller supplies already resolved rate windows and a
combined variable rate. Actual utility components, local-time/DST expansion,
bill proration, fixed charges/credits, loss multipliers, statement revisions,
storage, UI, authentication, telemetry, vehicle access and deployment remain
outside this first pricing milestone. No real rate or personal data is a fixture.
