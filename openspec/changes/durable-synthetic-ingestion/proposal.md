# Proposal

## Why

The receiver harness is intended to establish transport into Kafka, but an acknowledgment cannot preserve ChargeShare evidence across a consumer crash. The next [POC stage](../../../docs/plan.md#stage-2-durable-rust-ingestion) needs durable, replayable synthetic ingestion before the dashboard can use receiver-backed results.

## What Changes

- Propose an outer Rust Kafka consumer and normalizer that translates verified receiver output into the existing scoped core events.
- Require explicit fictional device-to-vehicle/owner mapping and a versioned synthetic evidence manifest for connection aliases, vehicle-wide event positions and boundary/AC annotations that the receiver does not supply.
- Retain accepted synthetic evidence, normalized event variants, provenance, safe rejection reasons and per-partition progress in one local SQLite transaction.
- Preserve exact decimal counters, missing readings, late evidence and same-position conflicts; distinguish broker delivery identity from domain deduplication.
- Rebuild the existing in-memory ledger from persisted events, proving independent 10 kWh / 4 kWh observed AC sessions without double counting across redelivery and restart.
- Specify bounded failure, interrupted-transaction recovery, schema/configuration compatibility, private runtime state and Linux acceptance tests through the actual receiver.

This PR is planning only, stacked on [PR #5](https://github.com/jestrada/ChargeShare/pull/5). Its published base is `fb6b2d3`; stage 1's decoded schema and transport acceptance remain prerequisites, not evidence supplied by this proposal. No ingestion code or database exists in this PR, and implementation tasks remain unchecked.

## Capabilities

### New Capabilities

- `durable-telemetry-ingestion`: Synthetic receiver normalization, scoped evidence retention, transactional progress and deterministic durable replay.

### Modified Capabilities

None. The accepted vehicle-ledger, pricing and dashboard contracts remain unchanged.

## Impact

A later approved implementation will add a cohesive outer Rust ingestion crate with narrow decoding, identity/manifest, SQLite and replay responsibilities, plus pinned Kafka/SQLite/serialization dependencies, migrations, fictional fixtures and an ingestion acceptance command. Tesla payload types, IO and SQL stay outside `chargeshare-core`; no core API change is planned.

Initial support follows stage 1: Linux x86_64, the pinned local receiver/Kafka stack and permitted host Docker. The SQLite file and companion files stay in ignored, isolated local runtime storage. Durable means recovery on the tested local filesystem, not backup, broker replication or cloud durability.

Non-goals: persisted pricing/API/dashboard reads (stage 3), classification editing or persistence, production authentication, real vehicle mapping/data retention, OAuth/key pairing, payments, public ingress, deployment and Cloudflare/Terraform (stage 4). The manifest labels its extra information as synthetic annotations; it does not prove real vehicle boundary semantics or utility-meter accuracy. Applying, archiving or merging needs the later reviewed workflow and its authorization.
