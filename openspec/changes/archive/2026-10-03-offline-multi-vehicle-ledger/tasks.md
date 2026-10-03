# Tasks

Implementation approved and applied on 2026-10-02. The original planning/review
context below is retained as history; current behavior is documented in
[the offline domain contract](../../../../docs/architecture.md#implemented-offline-ledger). Completion is
tracked by the checked tasks and [verification](../../../../docs/testing.md#spec-1-verification).

Proposed implementation checklist only. All work is unimplemented and gated on
review followed by a separate explicit implementation request. Completing the
planning artifacts or merging this spec PR does not authorize implementation.
There are no live setup tasks in this milestone. See the [testing plan](../../../../docs/testing.md)
for the current CI boundary, future receiver integration checklist and manual live gate.

## 1. Scoped offline inputs

- [x] 1.1 Add Rust domain types for synthetic owners, vehicles, connections and events; verify duplicate registration and missing/unknown identity tests leave state unchanged, and document accepted synthetic input semantics
- [x] 1.2 Add checked exact energy representation; verify negative, invalid, unsupported-precision and overflow inputs fail explicitly, and document supported precision

## 2. Session replay

- [x] 2.1 Implement vehicle-partitioned replay and deduplication with stable connection/session identities; verify interleaved, shuffled, duplicate, same-position conflict and identical-cross-vehicle fixtures, and document ordering and conflict policy
- [x] 2.2 Implement conservative AC deltas and connection grouping; verify pause/resume, new connections, DC/ambiguous type, rollback, missing baseline/end and silence scenarios with explicit flags, and document synthetic versus real-telemetry limits

## 3. Energy accounting isolation

- [x] 3.1 Add scoped session reads and manual charger classification; verify a cross-vehicle review fails without mutation and confirmation cannot clear evidence flags, and document that scope checks are not authentication
- [x] 3.2 Add separate per-vehicle observed and eligible kWh summaries with held/excluded reasons; verify the 10 kWh / 4 kWh example and unconfirmed defaults, and document that no money owed or certified measurement is produced

## 4. Spec 1 acceptance in GitHub Actions

- [x] 4.1 Add the complete synthetic replay suite to the normal Rust workspace test command; verify every Spec 1 scenario, exact separate 10 kWh / 4 kWh observed totals, independently confirmed eligible totals, and duplicate/out-of-order replay with stable session identities and flags, without network or credentials
- [x] 4.2 Run Rust formatting, clippy, workspace tests, strict OpenSpec validation and staged/history secret scans; inspect the exact diff and document actual results before requesting implementation review
- [x] 4.3 Verify GitHub Actions runs the complete offline suite on every pull request and push from a clean runner using the pinned Rust toolchain and lockfile; ensure no feature suite is silently ignored or skipped, and document the reproducible local command
- [x] 4.4 Verify an intentionally wrong expected total makes the test command and CI job fail (not continue-on-error), restore the correct assertion, and capture a passing run for the final commit; retain a safe test summary/failure artifact with no credentials or real data

## Later test stages, outside Spec 1

The [future integration checklist](../../../../docs/testing.md#future-receiver-integration-separate-spec)
requires fake-vehicle → real Go receiver → selected dispatcher → Rust tests in
GitHub Actions. It must become part of a separately reviewed integration spec;
these are not additional Spec 1 implementation tasks. Broker selection is deferred.
A real-car trial remains a separately approved manual validation, never a CI job.
