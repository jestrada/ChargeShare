# Design

Implementation approved and applied on 2026-10-02. The original planning/review
context below is retained as history; current behavior is documented in
[the offline domain contract](../../../../docs/architecture.md#implemented-offline-ledger). Completion is
tracked by the checked tasks and [verification](../../../../docs/testing.md#spec-1-verification).

## Context

See [proposal](proposal.md). The current Rust core is intentionally empty and has
no models, persistence, API or authentication to migrate. The previous broad OpenSpec change is deleted. This design is proposed, not implemented.

## Goals / Non-Goals

**Goals:** Make multi-vehicle isolation testable at the domain boundary before
adding transport or user accounts. Keep the first implementation small enough to
review with a two-vehicle acceptance fixture.

**Non-Goals:** Production owner authorization, owner transfers, billing, storage
recovery, Tesla payload parsing or an internet-facing endpoint. Synthetic owner
association supports future design; it is not proof of permission to view data.

## Decisions

1. **Rust core with explicit scoped values.** Model synthetic owner alias, vehicle
   alias, connection/session identity, event position, event time, counter value,
   charge type, quality flags and review state. Never use a process-global active
   vehicle. A single global counter or session map risks mixing adjacent events.
2. **In-memory replay first.** Fixtures and deterministic pure domain operations
   are sufficient to establish behavior. SQLite, durable ingestion and crash
   recovery belong to a later spec, avoiding infrastructure before semantics.
3. **Explicit synthetic connection boundaries.** Fixtures supply connection start,
   pause/resume and end evidence, plus whether an energy sample is a valid start
   baseline or terminal sample. The processor does not guess Tesla's production
   boundaries, resets or timing from silence. A rollback holds the session rather
   than inferring an extra billable segment.
4. **Exact energy arithmetic.** Parse finite, nonnegative kWh values into a checked
   fixed-point or decimal representation; reject unsupported precision and
   overflow explicitly rather than rounding silently. Pin/document the chosen
   supported precision during implementation and test its boundary. Do not use
   binary floating-point for ledger accumulation. Currency is deferred.
5. **Vehicle-first replay and identity.** Partition before deduplication and sorting;
   include vehicle identity in every evidence/session key. Within each partition,
   order by event time and stable synthetic event position. Conflicting payloads
   at the same position hold the affected session. Derive stable session IDs from
   explicit connection identity so late events do not attach review decisions to
   a different session. Review inputs are keyed by vehicle plus connection ID.
6. **Separate totals and reasons.** Observed deltas are evidence, eligible kWh is
   a conservative subset, and held/excluded records retain reasons. Confirmation
   changes only charger classification. No payable total is produced.

## Risks / Trade-offs

- Synthetic semantics may differ from real telemetry → document the adapter gap;
  validate real counter and connection behavior in a separately approved spec
- Scoped domain calls can be mistaken for user security → no login or public
  endpoint here; require an authorization design before any real owner access
- Owner visibility is unsettled → proposed later default is own vehicles only;
  no charger-owner aggregate or cross-owner sharing is authorized by this draft
- Conservative holds reduce eligible energy → preferable to silently combining
  counters, inventing missing energy or claiming a settled bill

## Test execution plan

[Testing plan](../../../../docs/testing.md) defines the sequence: Spec 1 synthetic Rust
fixtures in GitHub Actions first, a separately reviewed containerized receiver
integration suite later, then separately approved manual real-car validation.
No integration implementation, production credential, vehicle access or workflow
change is included in this planning PR. The existing workflow runs the empty
Rust scaffold; new test coverage must be demonstrated after implementation.

## Migration Plan

This PR publishes planning only. Delete the original unimplemented change;
keep the new change as the only active proposal and leave
`openspec/specs/` empty until implementation is separately approved and verified.
After review, an explicit implementation request can authorize the tasks below.
No data migration, deployment or rollback operation is needed for this spec PR.
A later receiver design may use a cloud or home host with suitable public
reachability; no hosting choice is made here.
