# Spec 1: Offline multi-vehicle charging ledger

Implementation approved and applied on 2026-10-02. The original planning/review
context below is retained as history; current behavior is documented in
[the offline domain contract](../../../../docs/architecture.md#implemented-offline-ledger). Completion is
tracked by the checked tasks and [verification](../../../../docs/testing.md#spec-1-verification).

**Status: draft for review; no application behavior is implemented by this PR.**
Review this spec first. Implementation starts only after review and a separate,
explicit request to implement it. Work on one spec at a time.

## Why

More than one Tesla may use a shared charger. Before connecting any vehicle,
prove with fictional events that each vehicle's sessions, AC energy and accounting
eligibility remain separate, including when events arrive interleaved.

## What Changes

- Replace the broad single-vehicle plan with one bounded offline milestone
- Associate each synthetic vehicle with an owner alias and keep all event,
  session, review and energy-total operations scoped to that vehicle
- Reconstruct conservative AC sessions and per-vehicle energy summaries in Rust
- Demonstrate deterministic replay, duplicate handling and cross-vehicle isolation
- Keep unknown, incomplete, DC and unconfirmed sessions out of eligible totals
- Delete the old unimplemented change and start fresh; it is not archived as completed

## Capabilities

### New Capabilities

- `vehicle-ledger`: Offline identity scoping, session reconstruction and per-vehicle energy accounting from synthetic evidence

### Modified Capabilities

None. `openspec/specs/` is empty and the Rust crate is still an empty scaffold.

## Impact

This PR changes planning documents only. A later approved implementation would
add domain types and tests to `crates/chargeshare-core`, using no Tesla accounts,
network listener, database service or UI. No runtime dependencies are selected
by this spec. Existing source and security workflows stay intact.

Out of scope: live telemetry, OAuth, registration/key pairing, owner login or
invitations, charger-owner dashboards, cross-owner sharing, tariffs/currency,
CSV, monthly statements, payments, location collection, hosting and deployment.
Here, **accounting means separate kWh and eligibility records**, not money owed.

## Review decisions

1. Is offline separation of two or more vehicles the right first milestone?
2. Is energy-only accounting enough for this first step, leaving prices to a later spec?
3. Proposed privacy default for later access design: an owner sees only their own
   vehicles; charger-owner or cross-owner visibility would require a separately
   reviewed sharing policy. This is an assumption to review, not approved sharing
   or an authentication feature delivered in Spec 1.

No cloud-versus-home hosting decision is needed here. Either can be considered
for a later receiver spec if its public reachability and security requirements
can be met.
