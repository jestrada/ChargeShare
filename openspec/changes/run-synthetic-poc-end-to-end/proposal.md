# Proposal

## Why

The third PR in the local stack must turn durable synthetic evidence into inspectable charging results and make the complete service graph runnable. Starting processes or seeing a receiver acknowledgment does not prove that scoped persisted energy, review and pricing reach the dashboard.

## What Changes

- Extend the existing pinned Linux Nix/Tilt/Compose graph with one Rust runtime owning Kafka ingestion, embedded SQLite and the persisted API, plus an explicitly persisted dashboard; retain the independent fixture preview.
- Provide one-command all-services startup, protocol/application readiness, finite acceptance, restart, state-preserving stop and explicit ownership-scoped reset.
- Persist minimal scoped manual charger-classification revisions and immutable versions of the existing labeled public sample rate table. Reconstruct and price with the Rust core; show unconfirmed, stale-review, missing-rate and quality holds conservatively.
- Replace fixture-backed reads in persisted mode with explicitly scoped SQLite-derived API results carrying evidence, review, rate and calculation provenance. Keep frontend money arithmetic out of scope.
- Exercise the actual receiver → Kafka → ingestion → SQLite → scoped API/dashboard path, with recovery/adverse-evidence scenarios, a dedicated Linux CI gate and manual desktop/mobile UI smoke.

## Capabilities

### New Capabilities

- `persisted-synthetic-results`: Scoped durable charging review/pricing and its complete local synthetic service lifecycle and acceptance boundary.

### Modified Capabilities

None. Existing offline core and fixture-dashboard contracts remain intact; persisted mode has a separate API and explicit labels.

## Impact

Proposed changes affect outer Rust ingestion/storage and preview orchestration, versioned schema migration, API/frontend reads, Tilt/Compose lifecycle validation, developer documentation and integration CI. SQLite is an embedded file with WAL/SHM companions, not a server to start. Kafka remains private inside Compose; only the local APIs/dashboard are published on IPv4 loopback.

Stage 1's transport/trust/ACK boundary is verified on published parent `33a456894eefb1955a6dd28aadb997e1fc079852`: its deliberate assertion failure was observed before restored passing Linux acceptance, retained replay, restart/reset and cleanup. PR #6 is rebased onto that head. Stage 2's actual receiver input, source-epoch lifecycle and durable replay acceptance remain prerequisites in its active change; this proposal neither completes nor absorbs them.

Planning only: all implementation tasks are unchecked. No services have been started for this proposal. Excluded: live Tesla access, new external credentials, real utility calendars or bills, statements/payments, authentication, cross-owner sharing, deployment and Cloudflare/Terraform. Implementation, review, archive and merge require their respective later authorization and evidence.
