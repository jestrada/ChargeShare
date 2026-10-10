# Tasks

Implementation authorized 2026-10-04. Check only verified tasks. Linux x86_64
includes suitable cloud hosts; keep networks private and host listeners loopback. Other
platforms and live Tesla setup are excluded. Full acceptance:
[local-development](specs/local-development/spec.md) and
[harness](specs/synthetic-telemetry-harness/spec.md) specs.

## 1. Pinned shell and preflight

- [ ] 1.1 Pin flake/lockfile, matching Rust, Tilt, Docker/Compose, Node/npm and utilities; verify `nix develop` versions and repeated-entry lock stability.
- [x] 1.2 Add synthetic-only daemon/platform/port/binding preflight; test stopped daemon, unsupported platform, occupied port and unsafe binding without host changes.
- [ ] 1.3 Document Nix features, host/cloud daemon prerequisites, cold downloads and support matrix; execute entry commands and record tested architecture/runtime versions.

## 2. Managed services

- [ ] 2.1 Pin Kafka KRaft and official receiver versions, digests and notices; verify reproducible Linux builds/pulls.
- [ ] 2.2 Add checkout-isolated Compose/Tilt services; verify protocol/status readiness, bounded failure, loopback publishing and no profiler exposure.
- [ ] 2.3 Generate ignored test certificates; verify permissions, untrusted-client rejection, unchanged system trust and no keys in Git, Nix inputs or images.
- [ ] 2.4 Manage API/Vite with independent locked installs/builds; verify unchanged preview, logs/readiness and orphan-free stop.
- [ ] 2.5 Document resources, endpoints, logs and Tilt start/stop; execute commands and verify container/host shutdown retains broker state.

## 3. Receiver smoke scenarios

- [ ] 3.1 Integrate pinned upstream helpers and Kafka verifier without domain API changes; require authenticated receiver ACK and matching decoded Kafka output, rejecting ACK-only or direct-injection success.
- [ ] 3.2 Test fixed multi-vehicle complete/missing/duplicate/out-of-order fixtures against expected semantics; preserve omissions without invented energy/cost or exactly-once claims.
- [ ] 3.3 Isolate finite runs using partition-start offsets plus exclusive execution/supported markers; reject stale matches and compare two clean normalized results.
- [ ] 3.4 Expose manual `telemetry-smoke` with bounded diagnostics; verify triggering, missing output, broker outage and invalid authentication within documented deadlines.
- [x] 3.5 Document fixture ownership, ACK limits and transport boundary; verify reports/Tilt labels keep adapter/database/dashboard integration unimplemented.

## 4. Retention and reset

- [ ] 4.1 Document independent-position replay; stop/restart verifier and re-read retained fixtures, with no application crash-recovery claim.
- [x] 4.2 Add stop-first reset listing exact scope with a confirmation flag; test active-resource refusal, unrelated-project preservation and clean runtime regeneration.
- [ ] 4.3 Document retained stop/start versus destructive reset; compare two full reset/start/test paths for identical normalized expectations.

## 5. Integrated acceptance and review

- [ ] 5.1 Run both complete specs from a clean supported Linux checkout; retain synthetic architecture/runtime, timing, outcome and unsupported-configuration evidence.
- [x] 5.2 Pass strict OpenSpec, security guards/scans, offline Rust acceptance, fmt/Clippy and frontend build; review core independence and unchanged direct preview.
- [ ] 5.3 Add every-push/PR integration on fresh `ubuntu-24.04` x86_64 with host Docker and pinned Nix/Tilt/Compose. Explicitly run finite smoke after bounded protocol readiness; require ACK plus decoded Kafka match. Fail setup/readiness/assertion/timeouts; retain allowlisted diagnostics and always clean containers, host processes and temporary state. Prove controlled assertion/missing-output failure, restore success and record passing exact-commit hosted evidence. Coverage excludes exhaustive domain cases and later ingestion/SQLite/dashboard/Cloudflare.
- [ ] 5.4 After review/verification, sync only accepted deltas and archive on the implementation PR; verify canonical requirements and task evidence before separately authorized merge.
