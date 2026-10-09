# Tasks

Planning is ready for review. Every task below is future work and remains unchecked. Start apply only after explicit approval; the parent transport/schema gate must pass before integration acceptance.

## 1. Verify the parent boundary and pin the input contract

- [ ] 1.1 Record the published stage-1 implementation commit and passing actual-receiver/Kafka Linux acceptance; verify its exact decoded schema, trust, topic and lifecycle rather than using unpublished code or ACK alone.
- [ ] 1.2 Specify the inspected schema/field allowlist, exact timestamp conversion, source association and broker-runtime epoch binding; verify golden decoded fixtures and rejection cases, and document any minimal synthetic lifecycle addition before coding it.
- [ ] 1.3 Add the outer ingestion crate and locked Kafka/SQLite/serialization dependencies; verify clean compilation and unchanged core imports/API under formatting and Clippy checks.

## 2. Normalize explicit scoped synthetic evidence

- [ ] 2.1 Implement frozen identity mapping and synthetic manifest validation; verify ambiguous registrations/associations, unknown identity and key/payload disagreement fail without vehicle mutation, and document provenance.
- [ ] 2.2 Decode allowlisted fields into lossless core events; verify decimal limits, missing/invalid counters, unsupported timestamps/types, duplicate fields and safe rejected-text handling with focused tests.
- [ ] 2.3 Add explicit Start/Sample/Pause/Resume/End fixture annotations with vehicle-wide positions; verify absent observed records never create manifest-only events and Complete/reconnect/silence never supplies End.
- [ ] 2.4 Document the mapping/manifest/schema contract and counter-omission behavior; verify examples remain fictional and golden normalization equals direct core input without adding a new quality flag for interior omission.

## 3. Retain evidence and progress transactionally

- [ ] 3.1 Add versioned SQLite metadata, registrations, dispositions, event variants/provenance and progress schema; verify migration rollback/newer-version refusal, full-range decimal-TEXT round trips and configuration incompatibility without deletion.
- [ ] 3.2 Implement one owning writer with verified WAL/FULL settings and bounded storage deadlines; verify competing writer, busy/full/corrupt storage failures preserve prior progress and evidence.
- [ ] 3.3 Commit each disposition/evidence/event set with its partition's safe next position; verify pre-commit rollback preserves both and interrupted commit recovers atomic old-or-new state and unknown/poison input advances only with its durable sanitized rejection.
- [ ] 3.4 Separate delivery dedup from domain variants; verify same-delivery replay, identical events at different offsets, cross-vehicle equality, cross-connection same-position conflicts and changed-content coordinate failure without overwrite.
- [ ] 3.5 Document the ignored database/companion-file lifecycle and guarantee limits; verify permitted retention excludes arbitrary raw JSON, names, locations and rejected payload text.

## 4. Resume and reconstruct the existing domain view

- [ ] 4.1 Consume manually assigned synthetic partitions with automatic broker progress disabled and SQLite recovery seek; verify reconnect, crash after commit and forced reread preserve committed evidence without double counting.
- [ ] 4.2 Verify stream epoch/cluster-topic identity and retained offset bounds before resuming; test reset, missing epoch and out-of-range progress fail without auto-reset or database loss, and document recovery/reset steps.
- [ ] 4.3 Rebuild a scoped ledger from frozen registrations and all event variants; verify 10/4 kWh observed totals, unconfirmed/zero eligible energy, stable session IDs/flags and unknown/mismatched owner scope rejection after restart.
- [ ] 4.4 Add permutation/late-input, pause/resume/new-connection, missing boundary, rollback, DC/ambiguous and conflict replay coverage; verify final retained evidence matches the existing core reference and document the local replay command.

## 5. Verify the integrated synthetic stage

- [ ] 5.1 Extend the supported Linux acceptance path through the actual pinned receiver, Kafka, ingestion, SQLite and scoped replay; verify finite multi-vehicle fixtures and restart/crash boundaries with bounded deadlines and owned-resource teardown.
- [ ] 5.2 Add the ingestion CI gate and allowlisted summaries; verify setup/assertion/timeout failure fails the job, deliberately wrong expectations fail, restored expectations pass and forbidden payload/key/certificate text is absent from diagnostics.
- [ ] 5.3 Run unchanged workspace tests/offline wrapper, Rust formatting/Clippy, frontend build, strict OpenSpec, security guards, staged/history scans and link/diff review; record the exact final published commit and terminal hosted checks with unrun stages disclosed.
- [ ] 5.4 After review and verified implementation, sync only this accepted capability into canonical specs and archive this change on its PR; verify the synced contract, archive, final commit and checks before any separately authorized merge.

Live Tesla registration/OAuth/key pairing, real data, costs, deployment and Cloudflare remain excluded and require a separate proposal and authorization. No task here authorizes those actions or connects the fixture dashboard.
