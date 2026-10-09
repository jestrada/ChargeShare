# Proposal

## Why

The receiver harness is intended to establish transport into Kafka, but an acknowledgment cannot preserve ChargeShare evidence across a consumer crash. The next [POC stage](../../../docs/plan.md#stage-2-durable-rust-ingestion) needs durable, replayable synthetic ingestion before the dashboard can use receiver-backed results.

## What Changes

- Add an outer Rust Kafka consumer and normalizer that translates scoped synthetic receiver-shaped records into existing core events.
- Require explicit fictional device-to-vehicle/owner mapping and a versioned synthetic evidence manifest for connection aliases, vehicle-wide event positions and boundary/AC annotations that the receiver does not supply.
- Retain accepted synthetic evidence, normalized event variants, provenance, safe rejection reasons and per-partition progress in one local SQLite transaction.
- Preserve exact decimal counters, missing readings, late evidence and same-position conflicts; distinguish broker delivery identity from domain deduplication.
- Rebuild the existing in-memory ledger from persisted events, proving independent 10 kWh / 4 kWh observed AC sessions without double counting across redelivery and restart.
- Specify bounded failure, interrupted-transaction recovery, schema/configuration compatibility, private runtime state and Linux acceptance tests through the actual receiver.

Implementation was approved on 2026-10-09. [PR #6](https://github.com/jestrada/ChargeShare/pull/6) remains stacked on [PR #5](https://github.com/jestrada/ChargeShare/pull/5) at published base `fb6b2d3`. The outer crate now implements candidate normalization, atomic SQLite retention and scoped replay, with local/mocked tests. Recovered parent `ec1c892` supplies matching fixture provenance, but its implementation is still absent from PR #5's published branch. Actual receiver acceptance, broker epoch lifecycle and dedicated integration CI remain unchecked prerequisites; see [verification](../../../docs/durable-ingestion.md).

## Capabilities

### New Capabilities

- `durable-telemetry-ingestion`: Synthetic receiver normalization, scoped evidence retention, transactional progress and deterministic durable replay.

### Modified Capabilities

None. The accepted vehicle-ledger, pricing and dashboard contracts remain unchanged.

## Impact

The outer Rust ingestion crate has narrow decoding, identity/manifest, SQLite, Kafka and replay responsibilities, pinned dependencies, migrations, fictional fixtures and bounded commands. Tesla payload types, IO and SQL stay outside `chargeshare-core`; its API and source are unchanged.

Initial support follows stage 1: Linux x86_64, the pinned local receiver/Kafka stack and permitted host Docker. The SQLite file and companion files stay in ignored, isolated local runtime storage. Durable means recovery on the tested local filesystem, not backup, broker replication or cloud durability.

Non-goals: persisted pricing/API/dashboard reads (stage 3), classification editing or persistence, production authentication, real vehicle mapping/data retention, OAuth/key pairing, payments, public ingress, deployment and Cloudflare/Terraform (stage 4). The manifest labels its extra information as synthetic annotations; it does not prove real vehicle boundary semantics or utility-meter accuracy. Archive follows completed implementation acceptance and review; merging requires separate authorization.
