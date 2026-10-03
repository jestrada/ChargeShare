# Tasks

Planning is complete; implementation has not started. All tasks below require a
later explicit apply request. Live setup is excluded rather than implied by these
tasks: Tesla registration, OAuth, key pairing, public hosting and real telemetry
need a separate reviewed change and authorization.

## 1. Pinned shell and host preflight

- [ ] 1.1 Add the flake and lockfile with pinned Tilt, Docker CLI/Compose, the repository Rust version, Node/npm and required utilities; verify `nix develop` version output and no lockfile changes on repeated entry.
- [ ] 1.2 Add project-scoped preflight checks for daemon reachability, Linux-container capability, required ports and synthetic-only configuration; test stopped daemon, unsupported platform, occupied port and unsafe binding failures without changing host settings.
- [ ] 1.3 Document Nix features, host daemon/macOS VM prerequisites, cold-start downloads and the supported architecture matrix; verify the documented entry commands on each platform before marking it supported.

## 2. Minimal managed service graph

- [ ] 2.1 Select compatible immutable Kafka KRaft image and official receiver source/build inputs, recording versions, digests and upstream notices; verify reproducible builds or pulls for each supported architecture.
- [ ] 2.2 Add project-isolated Compose resources and Tilt orchestration for Kafka and receiver; verify usable protocol/status readiness, bounded failures and loopback-only published ports with no profiler exposure.
- [ ] 2.3 Generate local-only test CA/client/server material under ignored runtime storage; verify restricted permissions, failed untrusted-client handshake, no system trust changes and absence of keys from Git, Nix store inputs and image layers.
- [ ] 2.4 Add independent Tilt local resources for the existing Rust API and Vite, with locked dependency-install/build steps; verify existing preview behavior, discoverable logs and readiness, and no orphaned child process after stop.
- [ ] 2.5 Document resource names, local endpoints, logs, `tilt up` and `tilt down`; execute the commands as written and verify container and host listener shutdown preserves the project broker state.

## 3. Receiver transport smoke scenarios

- [ ] 3.1 Integrate the pinned upstream-compatible synthetic client/test primitives and decoded Kafka verifier without changing domain APIs; verify an actual authenticated receiver exchange produces the expected Kafka record rather than accepting direct broker injection as the test.
- [ ] 3.2 Add fixed fictional multi-vehicle fixtures and expected semantic output for complete, missing-reading, duplicate and out-of-order cases; test all outcomes without invented energy/cost or exactly-once claims.
- [ ] 3.3 Isolate each finite run with per-partition starting offsets and exclusive execution or supported run markers; verify stale records cannot satisfy expectations and two clean runs match the same normalized result.
- [ ] 3.4 Expose the manual `telemetry-smoke` Tilt resource and bounded per-stage diagnostics; verify explicit triggering, missing expected output, broker outage and invalid client authentication each produce their specified result within the documented timeout.
- [ ] 3.5 Document fixture ownership, acknowledgment limitations and the receiver-to-Kafka completion boundary; verify reports and Tilt labels state that adapter, database and dashboard integration remain unimplemented.

## 4. Retention and safe reset

- [ ] 4.1 Add a documented independent consumer-position replay check; stop/restart the verifier and verify retained fixture records can be read again without claiming application crash recovery.
- [ ] 4.2 Add a narrowly scoped reset command with stop-first checks, exact resource listing and an explicit confirmation flag; test active-resource refusal, unrelated-project preservation and clean regeneration of runtime state.
- [ ] 4.3 Document normal stop/start retention versus destructive synthetic reset; verify the full reset/start/test path twice produces identical normalized expectations.

## 5. Integrated acceptance and review

- [ ] 5.1 Run the complete local-development and synthetic-telemetry-harness scenarios from a clean checkout on the declared host matrix; retain a synthetic summary of versions, timings, outcomes and unsupported combinations.
- [ ] 5.2 Run strict OpenSpec validation, security guard tests/scans, existing offline Rust acceptance tests, formatting/Clippy and frontend build; review the final diff to verify the core remains independent and existing direct preview still works.
- [ ] 5.3 After implementation review and verification, sync only this change's accepted deltas and archive its artifacts on the implementation PR; verify the canonical requirements and archived tasks match actual completed evidence before any separately authorized merge.
