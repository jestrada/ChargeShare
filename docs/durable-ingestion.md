# Durable synthetic ingestion

Stage-2 ingestion merged in [PR #6](https://github.com/jestrada/ChargeShare/pull/6).
[Issue #27](https://github.com/jestrada/ChargeShare/issues/27) adds the owned source
epoch, bounded committed-offset reread and a shared receiver-backed acceptance
command. These mechanisms are implemented with focused tests. Actual
receiver → Kafka → ingestion → SQLite → scoped replay acceptance remains
unverified in this sandbox. Its Docker socket is inaccessible. The dedicated
hosted gate is staged in `ci-27-tmp/`, awaiting maintainer activation and runs.
The existing fixture dashboard stays independent. OpenSpec is paused.

## Input and lifecycle contract

The candidate decoded `V` shape comes from fictional stage-1 fixtures at receiver revision `bd076fe1494841707528449560c4a19d0d426da4`. All six records match recovered harness commit `ec1c8922c813e8af57ea931324466ccc5afba5d5`, including its ACK-deadline fixes. PR #5 merged verified harness head `33a456894eefb1955a6dd28aadb997e1fc079852`, with passing Linux transport, retained replay, restart/reset and cleanup. PR #6 subsequently merged ingestion onto that implementation. A validated manifest maps fictional broker key/`vin` and exact integral UTC `createdAt` to stable vehicle-wide position, connection, event kind and charge type. Those annotations are synthetic context. Counter values come only from observed `ACChargingEnergyIn.stringValue`; missing readings remain absent and invalid readings retain safe core reasons. A charging-complete value or silence never creates an End.

Mappings, annotations, time conversion and normalization versions are frozen with the database. Accepted storage is curated; arbitrary raw JSON, vehicle names, location fields and rejected text are never retained. Exact counters and full-range unsigned positions use canonical decimal TEXT. Replays return the current core's scoped synthetic, physically unvalidated sessions, with Unconfirmed classification.

## Focused fixture commands

Run `cargo test -p chargeshare-ingestion --locked` for normalization, SQLite recovery and focused mocked-broker/CLI checks. Run `cargo run -p chargeshare-ingestion --locked -- candidate-demo` for a repeatable candidate normalization/storage demo in ignored `target/durable-ingestion-fixture`. Existing differing fixture state is refused; it is never overwritten or automatically reset.

The inspected candidate records produce 0.5/0.25 kWh. Separate handwritten consumer/storage tests establish independent 10/4 kWh reconstruction and shuffled/duplicate arrival; these do not demonstrate receiver transmission. All sessions default to unconfirmed and eligible energy remains zero. Source annotations never contain expected counters.

The versioned parser binds the inspected field allowlist to typed decoding:
top-level `data`, `createdAt`, `vin`, `isResend`; field objects contain only
`key` and `value`. `VehicleName`, `BatteryLevel` and `ACChargingEnergyIn` use
`stringValue`; `DetailedChargeState` uses `detailedChargeStateValue` with only
`DetailedChargeStateCharging`/`DetailedChargeStateComplete`. Names and battery
values are discarded. Other fields/shapes and duplicate source fields are
rejected. Counters canonicalize exact numeric value to six-decimal TEXT through
the core parser. Input contract is `tesla-v-bd076fe-v1`, normalizer is
`chargeshare-normalizer-v1`, and timestamp conversion is `utc-seconds-v1`.
Only integral `YYYY-MM-DDTHH:MM:SSZ` is accepted, with Gregorian validation and
signed Unix-second conversion; fractions, offsets and leap seconds are refused.
Kafka's zero timestamp header is not event time. `dev/ingestion-manifest.json`
freezes the two device/owner/vehicle mappings and the three received source times
per vehicle to explicit Start/Sample/End, AC type, positions and a matching
connection alias. Completion state alone never creates End.

The acceptance command captures actual matched decoded output privately and
passes it through the normalizer to check source times and associations. No
actual receiver capture was possible here. The candidate fixture remains
explicitly unverified; a reviewed golden fixture from a successful real run is
still required. See [fixture provenance](telemetry-fixtures.md#decoded-ingestion-capture).

## Owned broker source epoch

Runtime initialization now creates an owned Kafka volume with an immutable
`dev.chargeshare.source-epoch` label and a private `source-epoch.json` marker.
The generated `synthetic-<UUID>` epoch binds this dataset to
`chargeshare_synthetic_V`, the pinned cluster ID and partition 0 starting at
offset 0. Preparation installs the reviewed manifest privately and refuses
changes. Ordinary Tilt stop/start preserves the label, marker, evidence and
SQLite position. Only explicit stopped owned reset removes this pair; next
initialization replaces the epoch. Initialization interrupted between volume
creation and marker publication refuses the incomplete pair on retry, requiring
explicit reset. It does not adopt unlabeled older volumes.

Missing marker, missing volume, mismatched epoch, incompatible frozen config and
retained-offset gaps refuse recovery without deleting SQLite evidence or seeking
latest. Ownership is checked before container work; the Rust driver checks the
marker, actual cluster metadata, partition set and retained watermarks before
resuming. SQLite remains the sole position authority. The pinned development
cluster ID is constant across resets, so the volume-bound epoch is necessary.
This assumes trusted local Docker and no hostile same-UID actor; manually
rewriting labels/markers or destroying/recreating topics is outside the guarantee.

Inside the private ephemeral runner, the bounded CLI operations include:

- `cargo run -p chargeshare-ingestion --locked -- consume .local-runtime kafka:9092 6 30`, run inside the reviewed private Compose network
- `cargo run -p chargeshare-ingestion --locked -- replay .local-runtime owner-a vehicle-a`, using the configured fictional aliases
- `chargeshare-ingestion reread .local-runtime kafka:9092 0 0 6 30`, explicitly rereading partition 0 offsets `[0, 6)`

`reread_bounded` requires finite explicit ranges already committed in SQLite and
still retained in Kafka, with a maximum of 10,000 records and 120 seconds. It
verifies each curated disposition against SQLite through an immutable Store
reference; it cannot insert evidence or advance durable progress. Missing,
uncommitted or expired input and changed curated dispositions fail safely.
Reread retains no raw input.

## Shared end-to-end acceptance

On a fresh owned runtime in supported Linux, use:

```sh
nix develop --no-update-lock-file -c bash scripts/dev/acceptance.sh all
```

The command starts Tilt, sends the finite complete fixture through the unchanged
receiver and consumes it with disposable Rust processes in the existing private
network. It requires six accepted deliveries/variants, scoped observed AC energy
of 0.500000/0.250000 kWh, one Unconfirmed session per vehicle and zero eligible
energy. It resumes between independent consumers, kills actual consumers at
explicit before/after-commit barriers, recovers old/new state, then processes
actual duplicate/out-of-order receiver traffic without duplicating energy.
It forces retained rereads, checks missing/mismatched identity and an incompatible
frozen manifest, restarts Tilt,
deletes only this disposable broker's retained records to verify a real gap
against a private progress-3 snapshot, checks that expired reread refuses, and
performs an explicit stopped reset followed by new-epoch initialization.

Changed epochs and missing markers must return `source: broker runtime identity
mismatch`; a valid manifest with a changed version must return `storage: frozen
configuration mismatch`. Each refusal requires exit 2, empty stdout and the exact
stderr line. The command restores the input and independently checks scoped
results, evidence counts and SQLite progress after each case.

The outer deadline is 40 minutes with 60 seconds for exit handling. Startup is
bounded by 90-second Tilt API and 900-second protocol readiness deadlines;
consumers have CLI watchdogs and 45-second runner limits. Receiver ACK remains
10 seconds. Owned teardown runs on success and failure, using exact project
ownership and ephemeral container run labels. Preexisting runtime state is
refused, preserved and never reset by this command. A successful run prints
`Result: PASS`; failures print a fixed safe category and `Result: FAIL`.
Only an allowlisted summary is public; captures, logs, snapshots and test trust
stay private. This command has not passed here or in the new hosted job.

`consume-probe` is an explicit acceptance-only CLI operation. It pauses the
actual driver immediately before or after its first transaction commit, writes
only a private boundary marker, and has a 30-second watchdog. The orchestrator
kills that exact owned container. These barriers prove rollback/committed
recovery, not that a kill happened within SQLite's commit instruction. The
separate existing large-commit-window tests still cover consistent old-or-new
recovery without claiming exact instruction timing.

The additive `consume_bounded_observed` API exposes only BeforeCommit/AfterCommit
notifications for these probes. A before callback failure rolls back; an after
callback failure reports failure with committed state. Normal consumption uses
a no-op observer. This narrow outer-boundary hook costs less than a second
consumer implementation and ensures the acceptance kill exercises the actual
transaction path. Callbacks must be bounded by the caller; the CLI watchdog
provides that bound for the deliberate pause. No core API or dependency changes.

The current parent Kafka broker exposes no host port and advertises `kafka:9092` only inside its private network. A host-side loopback listener is not assumed. Bootstrap and advertised metadata checks refuse other hosts; these application checks are not a malicious-broker network sandbox. The reviewed parent network remains the isolation boundary. The pinned Kafka client exposes cluster ID but no topic UUID; a configured unsupported topic ID is refused rather than ignored.

The CLI watchdog bounds the entire consume process, including native-client teardown. The library shares a deadline across broker operations, but cannot strictly bound native client destruction; callers requiring a hard wall-clock bound must isolate it in a watched process. Storage uses a separate 250 ms busy timeout. A deadline failure leaves recovery to SQLite's atomic old-or-new transaction state.

## Local persistence boundary

One process owns each database. SQLite evidence/dispositions and partition progress commit together under WAL, FULL synchronization, foreign keys and a bounded busy timeout. Known rollback leaves both unchanged; indeterminate interrupted commit recovers either old or committed state consistently. Duplicate delivery is idempotent; distinct variants at `(vehicle, position)` remain available to the core's conflict/hold rules.

Only ignored owned local runtime paths may contain the database, writer lock and WAL/SHM companions. Reset must be explicit and scoped to those synthetic files; storage corruption or retention loss never triggers automatic reset. Recovery tests establish local process/database behavior on the tested filesystem, not backup, replicated-broker or cloud durability.

On Linux, no-follow directory/file handles and inode checks reject symlinks, hardlinks, special files and non-private database companions. Ancestors must be owned by the user or trusted root and not writable by other users, apart from trusted root/user-owned sticky directories. The runtime and its files are private. These checks assume a trusted filesystem and no hostile process running as the same user; they are not a same-UID security sandbox.

## Focused verification

The workspace includes 134 Rust tests, including 73 ingestion tests: 21
normalization, nine persistence, eight recovery, fifteen mocked Kafka, thirteen
CLI and seven unit tests. Focused reread checks verify stable results/progress,
fixed rejections and refusal of changed/uncommitted input. Child-process tests
interrupt before/after commit and in a large commit window, checking consistent
old-or-new evidence/progress. They do not prove the kill occurred inside SQLite's
commit instruction. See [current verification](testing.md#issue-27-local-verification).

Run `bash scripts/testing/ingestion-suite.sh` for the complete workspace and a
guarded, allowlisted focused summary. `python3 scripts/testing/test_ingestion_guard.py`
proves missing/reduced/failed/ignored/filtered suites fail and Cargo failure
status is preserved. Python runtime/acceptance tests cover epoch and ownership,
assertions, setup/timeout failures, exact refusal reasons and cleanup. The staged
dedicated gate uses these checks plus the shared actual acceptance command.

## Remaining verification

- Run the actual pinned receiver → Kafka → ingestion → SQLite → scoped replay command on permitted Linux x86_64/Docker and commit a reviewed decoded golden fixture
- Activate and prove the staged dedicated CI gate, including hosted setup/assertion/timeout failure and separate restored passing runs
- Record exact published commit and terminal hosted checks; current local commits are unpublished
- Review and record acceptance evidence in the owning issue and PR before marking the work complete

Live Tesla access, persistent reviews, priced/API/dashboard results, deployment and Cloudflare remain separate work.
