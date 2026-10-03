# Design

## Context

See [proposal](proposal.md) for motivation and scope. Main currently has the pure
Rust ledger/pricing core, a loopback Rust preview API and React/shadcn dashboard.
`scripts/preview-dev.sh` launches the two preview processes; no receiver adapter,
Kafka consumer, persistent ledger, Nix flake or Tilt environment exists.

The [local-development delta](specs/local-development/spec.md) owns the developer
experience. The [harness delta](specs/synthetic-telemetry-harness/spec.md) owns the
transport acceptance boundary. Existing product contracts are unchanged.

## Goals / Non-Goals

**Goals:** Make the first integration boundary runnable with a small local service
graph and an explicit test result. Keep host setup separate from project tools.
Expose status and logs without requiring a Kubernetes cluster. Preserve inward
Rust domain dependencies and the existing preview's synthetic behavior.

**Non-Goals:** No normalization into core events, durable consumer offsets,
transactional SQLite storage, live dashboard feed, authentication product work,
vehicle provisioning, OAuth, hosting or real-car validation. A broker record is
not an accepted ledger event or evidence of utility-meter accuracy.

## Decisions

### 1. Nix owns tools; the host owns container execution

Commit `flake.nix` and `flake.lock`. Provide the selected Tilt, Docker CLI/Compose,
Rust toolchain consistent with `rust-toolchain.toml`, Node/npm, Bash, certificate
and verification utilities from pinned inputs. Resolve any package mismatch with
a pinned overlay/input; never silently use a different Rust version. Package
lockfiles and Cargo.lock stay authoritative for project dependencies. Expose a
version summary including Nix, tools, receiver revision and image digests.

Host prerequisites are Nix with `nix-command` and flakes enabled, a running
Docker daemon providing Linux containers, permission to access its socket, and
network access for the first locked dependency fetch. macOS additionally needs
a functioning Linux container VM, provided by the developer's chosen Docker
runtime. Nix does not install or start that host runtime, alter socket permissions,
or accept provider agreements. Document a tested host/runtime version matrix;
record x86_64 and aarch64 Linux/macOS results separately and do not label untested
architectures supported.

Entering the shell does not start services. `tilt up` performs project-scoped
preflight and prerequisite build/install resources using locked inputs. The
existing npm frontend dependency install becomes a visible one-shot dependency,
so developers do not need to discover extra per-service commands. Cold-start
network/cache needs and failures remain visible. Automatic lockfile upgrades,
host package installation and silent downloads of unpinned scripts are excluded.

Alternative: independent host installs have fewer Nix concepts but drift between
machines. Nix services or a Kubernetes cluster add a second lifecycle system or
cluster setup; neither is needed for this milestone.

### 2. Tilt orchestrates a minimal Compose graph

Use Tilt's `docker_compose` integration for a single-node Kafka in KRaft mode and
the official Tesla receiver. Pin a compatible Kafka image by digest and receiver
source revision/image build inputs; do not copy the upstream all-dispatcher test
stack or its mutable tags. Use an isolated project/network/volume name derived
from the checkout, with stable names within that checkout. Kafka is a local
single-broker development system, with no production high-availability claim.

A private broker listener serves receiver and verifier containers. Publish a
separate correctly advertised loopback listener only if host inspection needs it.
Publish the receiver's synthetic transport only on loopback if needed; keep
profiler/debug endpoints unpublished. Container listeners may bind inside the
private network; host published bindings must remain loopback. Tilt itself and
all existing preview host listeners remain loopback.

Manage the existing preview API and Vite as Tilt local resources with independent
logs, readiness and process cleanup. Build/install prerequisites precede their
serve commands. Keep the existing direct preview command available. Do not wrap
it in a way that leaves child processes behind or presents the pair as one
uninspectable process. Label both as the fixture demo, separate from telemetry.

Use protocol-level Kafka metadata checks and the pinned receiver's supported
status endpoint plus a synthetic authenticated handshake. Use bounded startup
and per-test deadlines documented alongside implementation. Compose dependency
health checks and Tilt resource dependencies express ordering; a TCP connection
alone is insufficient. Inspect actual selected-version behavior before choosing
health endpoints. Tilt's UI and CLI provide per-resource status and logs;
document concrete commands with the final resource names.

Alternative: run Kafka directly as a Nix process. Containerized Kafka and the
external Go receiver reduce host-specific native library/toolchain coupling.
Kubernetes adds no needed capability here. Additional brokers and observability
stacks are deliberately omitted.

### 3. Kafka is the receiver handoff, not a backend implementation

Select Kafka for the local dispatcher because the official receiver supports it
and the proposed handoff needs retained records and independent reader positions.
Redis Pub/Sub broadcasts to currently connected subscribers without a retained
stream for later replay. Redis Streams is a different API and cannot be obtained
by merely renaming a Pub/Sub configuration. This decision avoids building a
custom dispatcher before testing the receiver boundary.

Configure the pinned receiver for decoded JSON output and only the required
telemetry/connectivity record types. Set and verify its version-specific Kafka
acknowledgment policy rather than assuming a successful client exchange proves
backend persistence. Receiver acknowledgment, Kafka retention, and a future
atomic database/offset commit are separate guarantees. No production retention
or exact-once promise follows from this local setup.

### 4. Use upstream-compatible synthetic transport, not a fake webhook

Build the official receiver and its compatible synthetic test-client primitives
from a reviewed pinned upstream revision. Reuse protocol encoding/test support
with required license attribution rather than reimplement Tesla's transport.
If a small ChargeShare-specific test driver is necessary, keep it an outer Rust
harness; official upstream Go test tooling remains an external dependency.
Never create a new Go ChargeShare backend or put Tesla types in the core.

Generate a disposable test CA and client/server certificates at runtime in an
ignored project directory. Restrict private-file permissions, mount material
read-only where possible, and never put keys in Nix expressions/store outputs,
Git, image layers or public reports. Trust applies only to the synthetic client
and receiver; do not modify OS/browser trust stores or use insecure TLS flags.
These local test credentials do not authorize persistent access to an external
account. The environment cannot select live receiver configuration.

Expose an explicit manual Tilt test resource named `telemetry-smoke`, runnable
from the UI or `tilt trigger telemetry-smoke`. Starting services does not publish
an unlimited stream. A finite fixture uses fictional `device-1`/`device-2`-style
identities, fixed event timestamps and counter/state samples. Extend with named
missing-reading, duplicate and out-of-order fixtures. Record concrete expected
receiver output for the chosen revision, including its actual duplicate behavior;
do not assume transport delivery implies domain deduplication.

The verifier records per-partition starting offsets before sending and uses an
isolated consumer group/run marker where the protocol permits. If a run marker
cannot be preserved, use exclusive harness execution and compare records strictly
after the recorded offsets. Avoid random event times. Normalize only documented
volatile envelope fields, retaining identity, source time, field validity and
values. Compare per-vehicle semantic multisets or defined partition order rather
than global arrival order. Negative tests use the same isolation and explicit
observation deadlines so old records cannot hide a failure.

A passing report means synthetic client -> authenticated receiver -> decoded
Kafka output only. A direct Kafka fixture producer is useful for a future Rust
adapter test but cannot pass this milestone's receiver test.

### 5. Stop preserves state; reset is explicit and narrow

Document `tilt down` as the stop action. Exiting `tilt up` alone leaves Compose
services running. Verify stop behavior for host serve processes as well as
containers. Use a named project volume for retained Kafka data.

Provide a separately named reset command that refuses to run while this
checkout's services are active, prints its exact synthetic scope, and requires
an explicit confirmation flag before deletion. It removes only this project's
broker volume, test offsets and generated runtime files. It never uses global
Docker prune, deletes source, resets browser settings or touches other checkouts.
Certificates regenerate after reset. Ordinary stop/start retains state; clean
reset restores reproducible fixture expectations.

## Risks / Trade-offs

- Upstream native dependencies and container architecture support differ -> pin
  the full build closure and verify target platforms; report unsupported combinations.
- Nix bootstrap and Docker VM remain prerequisites -> provide one concise host
  setup checklist and actionable doctor output; never claim zero-install setup.
- Single-node Kafka can lose data -> use it only for local synthetic verification;
  defer production durability and recovery contracts.
- Transport authentication is not a production vehicle allowlist -> synthetic
  trust alone is sufficient only for this isolated harness; live ingress retention
  and allowlist enforcement require a separate reviewed security contract.
- Port conflicts or stale state can create misleading results -> strict binding,
  instance ownership, protocol probes and run offset isolation.
- Exact upstream helper compatibility is not yet executed -> implementation must
  verify pinned test-client APIs and decoded schema before recording acceptance.

## Migration Plan

This is additive developer tooling with no database or product migration. Keep
existing direct preview and offline test workflows usable. Implement only after
separate approval, verify the local-development and harness scenarios, then
update setup/testing documentation. Roll back by stopping Tilt and removing the
new tooling entry points; explicitly reset only disposable synthetic state.
Do not archive this change or sync it into the canonical contract until its
implementation is reviewed and verified.

## Source Verification

Official Tilt Docker Compose guidance, Tiltfile API reference, Nix `develop` and
flake manuals, and the Tesla Fleet Telemetry README, Compose configuration,
Makefile and integration configuration were inspected on 2026-10-03. They support
the orchestration and transport approach, not an executed compatibility claim.
Version numbers, source hashes, image digests and host support evidence must be
recorded together during implementation. No Nix/Docker installation, service
startup or receiver test was performed while preparing this proposal.
