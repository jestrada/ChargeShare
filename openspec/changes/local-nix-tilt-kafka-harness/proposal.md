# Proposal

## Why

ChargeShare needs a repeatable local environment before its receiver-to-ledger integration can be developed. A pinned Nix shell and Tilt-managed services let a developer exercise the official Tesla receiver with synthetic traffic and inspect Kafka without assembling infrastructure by hand.

## What Changes

- Propose `nix develop` followed by `tilt up` as the normal startup path on Linux machines, including suitable cloud Linux environments, after installing Nix and a working host Docker daemon.
- Pin development tools and upstream container inputs; use Tilt with Docker Compose, without requiring Kubernetes or new cloud accounts.
- Run a local Kafka broker and the official Tesla Go receiver, with a deterministic synthetic sender and a bounded Kafka output verifier.
- Include the existing Rust preview API and React dashboard in Tilt as separately labeled synthetic demo resources. Their data stays independent of receiver traffic in this milestone.
- Define readiness, actionable failures, per-service logs, explicit shutdown, isolated runtime files and a deliberate synthetic-state reset.
- Keep Kafka as the chosen local dispatcher. Defer the Rust normalization/consumer adapter, durable ledger storage, receiver-backed dashboard and all live Tesla setup.

This change contains planning artifacts only. The shell, service definitions and harness are proposed, not implemented. No production reliability or utility-meter accuracy is claimed.

## Capabilities

### New Capabilities

- `local-development`: Reproducible local toolchain, service lifecycle, diagnostics and isolated synthetic runtime state.
- `synthetic-telemetry-harness`: Reproducible synthetic transport through the official receiver into Kafka, with observable verification and failure boundaries.

### Modified Capabilities

None. Existing ledger, pricing and dashboard behavior remains unchanged.

## Impact

Future implementation will add a root flake and lockfile, Tilt/Compose definitions, narrowly scoped local helpers, public-safe fixture expectations and local-development documentation. It may wrap the existing preview launch path but does not change domain APIs or add Kafka/Tesla dependencies to `chargeshare-core`.

Initial support is Linux machines, including suitable cloud Linux environments; support for other operating systems is deferred. Host prerequisites remain Nix with flakes and `nix-command` enabled, a running Docker-compatible daemon with Linux-container support, permission to use it, and first-run network access for pinned dependencies. Nix provides developer tools, not the host daemon. Record the tested Linux architecture and runtime during implementation rather than claiming support for untested configurations. Cloud execution must provide the same permitted runtime and preserve private networking and loopback listeners; cloud provisioning, public ingress and deployment remain outside scope.
