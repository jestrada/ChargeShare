# ChargeShare proof of concept plan

Build three synthetic local stages, then validate Cloudflare using Terraform.
Each needs reviewed acceptance; this roadmap authorizes no implementation,
deployment, credentials, spending or vehicle access.

Offline ledger/pricing, fixture preview and stage 1's synthetic receiver/Kafka
harness are implemented and verified. Stage 2 has independently tested candidate
normalization, SQLite recovery and mocked Kafka replay; its actual receiver-backed
ingestion acceptance remains pending. Persisted results and Cloudflare need
separate changes. See [architecture](architecture.md), [pricing](pricing.md) and
[local preview](local-preview.md) for existing behavior.

## Route to a working POC

```mermaid
flowchart LR
    baseline["Implemented baseline<br/>Offline core + fixture preview"]
    harness["1 Verified receiver harness<br/>Nix + Tilt + Kafka"]
    ingestion["2 Partially applied ingestion<br/>Rust adapter + SQLite"]
    results["3 Proposed end-to-end POC<br/>Persisted core + API + dashboard"]
    cloudflare["4 Future Cloudflare POC<br/>Architecture + Terraform"]
    baseline --> harness --> ingestion --> results --> cloudflare
```

Arrows show milestone order. Local stages target Linux x86_64 with permitted
Docker, including private cloud hosts. Cloudflare follows working local stages;
Linux success establishes no Cloudflare compatibility.

Keep scoped, exact results across replay/restart; show missing/conflicting
evidence as holds. Synthetic results establish no real bill or meter accuracy.

## Ownership

| Component | Owner / ChargeShare responsibility |
| --- | --- |
| Official receiver / dispatcher | Tesla; pin and configure, preserve upstream transport. |
| Synthetic client | Upstream helpers; adapt finite fictional fixtures. |
| Kafka | Apache; configure retained handoff, no managed provider selected. |
| SQLite | Upstream; own schema, migrations, transactions and lifecycle. |
| Rust integration / core / API / dashboard | ChargeShare; normalize, replay, price and present scoped evidence. |
| Cloudflare / Terraform provider | Upstream; review topology and versioned infrastructure. |

Transport, storage and presentation stay outside the pure Rust core. Receiver
ACK and decoded records do not establish an accepted ledger event or database
commit. Existing behavior is documented in [architecture](architecture.md),
[pricing](pricing.md) and [local preview](local-preview.md).

## Stage 1 Receiver transport harness

Completed and archived on 2026-10-10: [proposal](../openspec/changes/archive/2026-10-10-local-nix-tilt-kafka-harness/proposal.md),
[design](../openspec/changes/archive/2026-10-10-local-nix-tilt-kafka-harness/design.md),
[tasks and evidence](../openspec/changes/archive/2026-10-10-local-nix-tilt-kafka-harness/tasks.md).
Canonical contracts are [local development](../openspec/specs/local-development/spec.md)
and [synthetic telemetry harness](../openspec/specs/synthetic-telemetry-harness/spec.md).
[Testing](testing.md) records complete fresh Linux acceptance, failure proof,
retained restart/reset and cache timings.

Pinned Nix/Tilt/Compose manages receiver, Kafka and separate fixture preview.
**Acceptance:** Fresh Linux protocol readiness, finite mTLS WebSocket exchange,
ACK **and** matching decoded Kafka fields within deadlines. Reject invalid
trust/stale matches; verify stop/reset. The fresh `ubuntu-24.04` gate fails
setup/assertions/timeouts, cleans up and retains safe diagnostics. ACK-only/direct
injection cannot pass. Ingestion, persistence and telemetry-backed results remain
outside stage 1.

## Stage 2 Durable Rust ingestion

**Status:** Approved and partially applied in [draft PR #6](https://github.com/jestrada/ChargeShare/pull/6),
stacked on [PR #5](https://github.com/jestrada/ChargeShare/pull/5), with a separate
[durable ingestion change](../openspec/changes/durable-synthetic-ingestion/proposal.md). Its [design](../openspec/changes/durable-synthetic-ingestion/design.md),
[detailed contract](../openspec/changes/durable-synthetic-ingestion/specs/durable-telemetry-ingestion/spec.md)
and [tasks](../openspec/changes/durable-synthetic-ingestion/tasks.md) track the accepted scope. Candidate normalization,
atomic SQLite evidence/progress and scoped replay have focused tests; see the
[implementation guide](durable-ingestion.md). Stage 1's published head `33a4568`
has verified receiver/Kafka transport, retained replay, restart/reset and cleanup.
The owned broker epoch lifecycle and actual receiver-backed ingestion remain
prerequisites for stage-2 end-to-end acceptance.

The outer Rust consumer/normalizer implements configured synthetic identity mapping,
evidence retention, SQLite schema, deduplication, late/conflicting messages and
transactional consumer progress under the reviewed spec.
Keep upstream payload types and database details out of the core. Decide how
retained transport evidence reconstructs the domain's explicit session boundaries
without inventing samples or treating silence as completion. The proposed local
synthetic manifest supplies explicit connection/position/boundary annotations
that receiver records do not provide; it does not establish real vehicle
boundary semantics. Stage 2 retains observed evidence and defaults reconstructed
sessions to unconfirmed. Persisted review decisions and results remain stage 3.

**Acceptance:** Fixed multi-vehicle records map into valid scoped core events;
duplicates and reordered/late input replay deterministically without double
counting. Missing, invalid and conflicting readings remain visible. Demonstrate
reconnect and restart at transaction/consumer-progress boundaries, including a
crash between reading and committing, with no lost accepted evidence or inflated
energy. Review safe diagnostics and unknown-identity rejection.

**Boundary:** Durable synthetic ingestion and replay only. Production allowlists,
real telemetry retention, account authentication and vehicle provisioning need
their own reviewed contracts before any live input.

## Stage 3 Run the persisted synthetic POC end to end

**Status:** Proposed as the third PR in the local stack, based on stage-2
[PR #6](https://github.com/jestrada/ChargeShare/pull/6), in the separate
[run-synthetic-poc-end-to-end change](../openspec/changes/run-synthetic-poc-end-to-end/proposal.md).
Its [design](../openspec/changes/run-synthetic-poc-end-to-end/design.md),
[contract](../openspec/changes/run-synthetic-poc-end-to-end/specs/persisted-synthetic-results/spec.md)
and [tasks](../openspec/changes/run-synthetic-poc-end-to-end/tasks.md) are planning only.
No all-services or receiver-backed dashboard acceptance is claimed.

Extend the existing pinned Linux Nix/Tilt/Compose graph with one Rust runtime
inside the private Kafka network, owning ingestion, the embedded SQLite file and
the scoped persisted API, plus an explicitly persisted dashboard. Preserve the
independent fixture API/dashboard. Provide one-command startup/readiness,
explicit finite check, restart, state-preserving stop and narrowly confirmed reset.
SQLite has no separate server to start. Stage 1's transport/trust/ACK acceptance
is verified at `33a4568`. Stage 2's broker-epoch and actual receiver-backed
ingestion/durable replay acceptance remain prerequisites in their original
change; its incomplete tasks do not move here.

Calculate sessions, observed/eligible/priced energy and exact subtotals through
the existing Rust core from committed curated evidence. Persist minimal scoped
manual review revisions and immutable public-sample rate versions with explicit
synthetic time conversion. Default sessions remain Unconfirmed; explicit review
cannot clear quality holds, and changed evidence makes earlier review stale until
reconfirmed. Display missing readings/rates, conflicts and source/review/rate
provenance. The frontend does no money arithmetic or fixture fallback.

**Acceptance:** A fresh supported Linux checkout starts every configured resource
through the documented locked command and passes protocol/application readiness.
A separately triggered finite test proves actual receiver ACK → new Kafka records
→ committed SQLite evidence/progress → scoped API and persisted-dashboard results.
Verify unconfirmed zero-eligible results before explicit scoped review/pricing,
real crash/restart, duplicate/reordered/late input, conflict/missing-data holds,
retention-gap refusal and safe owned teardown/reset. A dedicated Linux CI gate
must fail with a deliberately wrong expectation, then pass after restoration on
the recorded exact commit. Separately execute desktop/mobile UI smoke; readiness,
fixture previews and API/build checks do not substitute for that evidence.

**Boundary:** A working local synthetic review/pricing POC. Real utility calendars,
bills, payments/statements, production authentication, cross-owner sharing, live
Tesla access and deployment remain separate. The final Cloudflare/Terraform stage
needs its own architecture and actual platform acceptance.

## Stage 4 Cloudflare architecture and Terraform deployment

After local success, propose architecture/security and deployment separately.
Validate the actual runtime first:

- Image/CPU support, lifecycle, resource limits and cold starts
- WebSocket/mTLS ingress preserving authenticated client-certificate identity
- Private broker/consumer connectivity and access control
- Durable evidence, offsets and SQLite/storage recovery; local disk is not assumed durable
- Private authenticated UI/API, scope, secrets, observability, retention, costs and rollback

Pin Terraform/provider versions and review the plan before apply. Verify provider
resource support; document application/image deployment separately. Keep protected
state and credentials outside this public repository.

**Acceptance:** After explicit deployment authorization, exercise actual Cloudflare
ingress and the complete persisted path: ACK, correct scoped results, rejection
and platform restart/recovery. Verify repeatable Terraform configuration, safe
rollback and agreed resource/cost limits. Resolve unsupported ingress/storage
before completion. This remains synthetic; live Tesla setup and accuracy or
reimbursement validation need separate authorization.

## Review and evidence

Link future specs/PRs; record exact commits, pins, acceptance and unsupported
cases. Review, verify, sync and archive before authorized merge. Official upstream
guidance establishes no executed compatibility or chosen Cloudflare topology.
