# Durable synthetic ingestion

The approved stage-2 implementation is on [PR #6](https://github.com/jestrada/ChargeShare/pull/6), stacked on [PR #5](https://github.com/jestrada/ChargeShare/pull/5). Local normalization/storage tests use candidate decoded receiver fixtures; actual receiver transport acceptance remains unrun. The existing fixture dashboard stays independent.

## Input and lifecycle contract

The candidate decoded `V` shape comes from fictional stage-1 fixtures at receiver revision `bd076fe1494841707528449560c4a19d0d426da4`. All six records match recovered harness commit `ec1c8922c813e8af57ea931324466ccc5afba5d5`, including its ACK-deadline fixes. PR #5's branch still publishes only `fb6b2d3`; no parent history was rewritten. A validated manifest maps fictional broker key/`vin` and exact integral UTC `createdAt` to stable vehicle-wide position, connection, event kind and charge type. Those annotations are synthetic context. Counter values come only from observed `ACChargingEnergyIn.stringValue`; missing readings remain absent and invalid readings retain safe core reasons. A charging-complete value or silence never creates an End.

Mappings, annotations, time conversion and normalization versions are frozen with the database. Accepted storage is curated; arbitrary raw JSON, vehicle names, location fields and rejected text are never retained. Exact counters and full-range unsigned positions use canonical decimal TEXT. Replays return the current core's scoped synthetic, physically unvalidated sessions, with Unconfirmed classification.

## Focused fixture commands

Run `cargo test -p chargeshare-ingestion --locked` for normalization, SQLite recovery and focused mocked-broker/CLI checks. Run `cargo run -p chargeshare-ingestion --locked -- candidate-demo` for a repeatable candidate normalization/storage demo in ignored `target/durable-ingestion-fixture`. Existing differing fixture state is refused; it is never overwritten or automatically reset.

The inspected candidate records produce 0.5/0.25 kWh. Separate handwritten consumer/storage tests establish independent 10/4 kWh reconstruction and shuffled/duplicate arrival; these do not demonstrate receiver transmission. All sessions default to unconfirmed and eligible energy remains zero. Source annotations never contain expected counters.

The versioned parser accepts only the inspected typed candidate fields, with `DetailedChargeStateCharging`/`DetailedChargeStateComplete` state values. Valid counters canonicalize exact numeric value to six-decimal TEXT through the existing core parser. Input contract is `tesla-v-bd076fe-v1`, normalizer is `chargeshare-normalizer-v1`, and timestamp conversion is `utc-seconds-v1`.

## Broker runtime epoch prerequisite

The verified stage-1 lifecycle must supply an owned `source-epoch.json` marker in its private `.local-runtime` directory. Before ingestion starts, that marker identifies a new fictional epoch for the broker dataset, the fixed `chargeshare_synthetic_V` topic, actual cluster identity where available and explicit initial partition offsets. Broker reset must generate a new epoch; routine process restart must preserve it. Ingestion refuses a missing/mismatched marker, changed frozen configuration or retained-offset gap without deleting the database or seeking newest input.

This minimal lifecycle addition is specified here before implementation. The published parent branch does not yet provide it. Generating a standalone epoch for a focused fixture test is not proof that a real broker lifecycle preserves this binding. Parent publication and actual receiver/Kafka verification remain prerequisites for end-to-end ingestion acceptance.

For the eventual verified private runtime, the bounded commands are:

- `cargo run -p chargeshare-ingestion --locked -- consume .local-runtime kafka:9092 6 30`, run inside the reviewed private Compose network
- `cargo run -p chargeshare-ingestion --locked -- replay .local-runtime owner-a vehicle-a`, using the configured fictional aliases

The current parent Kafka broker exposes no host port and advertises `kafka:9092` only inside its private network. A host-side loopback listener is not assumed. Bootstrap and advertised metadata checks refuse other hosts; these application checks are not a malicious-broker network sandbox. The reviewed parent network remains the isolation boundary. The pinned Kafka client exposes cluster ID but no topic UUID; a configured unsupported topic ID is refused rather than ignored.

The CLI watchdog bounds the entire consume process, including native-client teardown. The library shares a deadline across broker operations, but cannot strictly bound native client destruction; callers requiring a hard wall-clock bound must isolate it in a watched process. Storage uses a separate 250 ms busy timeout. A deadline failure leaves recovery to SQLite's atomic old-or-new transaction state.

## Local persistence boundary

One process owns each database. SQLite evidence/dispositions and partition progress commit together under WAL, FULL synchronization, foreign keys and a bounded busy timeout. Known rollback leaves both unchanged; indeterminate interrupted commit recovers either old or committed state consistently. Duplicate delivery is idempotent; distinct variants at `(vehicle, position)` remain available to the core's conflict/hold rules.

Only ignored owned local runtime paths may contain the database, writer lock and WAL/SHM companions. Reset must be explicit and scoped to those synthetic files; storage corruption or retention loss never triggers automatic reset. Recovery tests establish local process/database behavior on the tested filesystem, not backup, replicated-broker or cloud durability.

On Linux, no-follow directory/file handles and inode checks reject symlinks, hardlinks, special files and non-private database companions. Ancestors must be owned by the user or trusted root and not writable by other users, apart from root-owned sticky temporary directories. The runtime and its files are private. These checks assume a trusted filesystem and no hostile process running as the same user; they are not a same-UID security sandbox.

## Focused verification

The complete workspace has 115 passing Rust tests, including 64 ingestion tests: 20 normalization, nine persistence, eight recovery, ten mocked Kafka, ten CLI and seven unit tests. Child-process tests interrupt before/after commit and in a large commit window, checking consistent old-or-new evidence/progress. They do not prove the kill occurred inside SQLite's commit instruction. Repeated parallel recovery runs pass after explicit writer-lock release.

Run `bash scripts/testing/ingestion-suite.sh` for the complete workspace and a guarded, allowlisted focused summary. `python3 scripts/testing/test_ingestion_guard.py` proves missing/reduced/failed/ignored/filtered suites fail and Cargo failure status is preserved. This local wrapper is not yet a dedicated hosted receiver integration gate. See [testing](testing.md) for publication checks.

## Remaining verification

- Publish and verify the exact parent harness implementation/schema/lifecycle without losing its later ACK-deadline fixes
- Run the actual pinned receiver → Kafka → ingestion → SQLite → scoped replay path on permitted Linux x86_64/Docker
- Add and prove a dedicated ingestion CI gate, with safe summaries and teardown on failure
- Review, then sync/archive this change only after its implementation acceptance is complete

Live Tesla access, persistent reviews, priced/API/dashboard results, deployment and Cloudflare remain separate work.
