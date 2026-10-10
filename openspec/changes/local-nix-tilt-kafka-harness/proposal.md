# Proposal

## Why

Give developers one repeatable way to test synthetic traffic through the official
Tesla receiver into Kafka before building ledger ingestion.

## What Changes

- Start with `nix develop`, then `tilt up` on Linux x86_64, including suitable cloud Linux hosts.
- Pin tools and container inputs; manage Kafka and the unpatched official receiver through Tilt/Compose.
- Verify finite, deterministic fixtures with both receiver acknowledgment and matching decoded Kafka output.
- Manage the existing API/dashboard as separate fixture-demo resources; receiver traffic does not feed them.
- Provide bounded readiness, per-service logs, explicit shutdown and isolated, confirmed synthetic reset.
- Run the same transport gate on fresh `ubuntu-24.04` GitHub Actions runners, with safe diagnostics and guaranteed teardown.

Local implementation is in progress; full receiver/Kafka acceptance remains
unverified. [Tasks](tasks.md) track completion. Rust ingestion, durable storage,
receiver-backed results and Cloudflare/Terraform belong to later
[roadmap stages](../../../docs/plan.md). Live Tesla setup, deployment and accuracy
claims are excluded.

## Capabilities

### New Capabilities

- `local-development`: Pinned tools, service lifecycle, diagnostics and isolated runtime state.
- `synthetic-telemetry-harness`: Synthetic receiver-to-Kafka verification and failure boundaries.

### Modified Capabilities

None; ledger, pricing and dashboard behavior stays unchanged.

## Impact

Adds flake/lockfiles, Tilt/Compose definitions, local helpers, fixtures, CI and
[setup documentation](../../../docs/local-development.md). The Rust core gains
no Tesla/Kafka dependencies or API changes.

Hosts need Nix flakes, `nix-command`, permitted Linux-container Docker access
and first-fetch network access. Nix supplies tools, not the daemon. Keep private
networking and loopback listeners. Other platforms, cloud provisioning and
Cloudflare compatibility remain outside stage 1; record tested runtime evidence
before claiming support.
