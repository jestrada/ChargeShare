# Persisted synthetic results delta

## Purpose

Make the complete local synthetic charging path runnable and expose scoped durable review/pricing results without hiding uncertainty, stale evidence or incomplete recovery.

## ADDED Requirements

### Requirement: Complete local service lifecycle
The system SHALL provide one documented locked entry command starting all receiver, broker, ingestion/API and dashboard resources on supported Linux x86_64. SQLite SHALL be an embedded owned file. Existing fixture resources SHALL remain explicitly independent. Startup SHALL not implicitly send an unbounded fixture stream or claim end-to-end success.

#### Scenario: One-command startup
- **WHEN** a fresh checkout with permitted Nix and a local Docker daemon runs the documented all-services command
- **THEN** dependency installation/build and every configured resource are visible under one supervisor with separate status/logs
- **AND** persisted and fixture URLs/modes are identified without manual per-service startup

#### Scenario: Unsupported or blocked host
- **WHEN** platform, Docker access, pinned dependency fetch, port availability or private runtime ownership fails
- **THEN** startup fails at the named boundary within its documented deadline and reports an actionable safe reason
- **AND** no host permissions, security settings or external credentials are changed

### Requirement: Readiness is distinct from acceptance
Readiness SHALL verify receiver protocol, Kafka metadata, runtime storage/source compatibility, scoped API access and the dashboard's intended read path under bounded deadlines. A separate explicit finite acceptance action SHALL prove the full path. A listening port, running process or fixture preview SHALL NOT establish persisted-path success.

#### Scenario: Ready services without evidence
- **WHEN** every resource becomes ready but no acceptance fixture has been sent
- **THEN** status reports readiness only and the empty persisted view is distinguishable from a completed charging calculation

#### Scenario: One broken dependency
- **WHEN** the API answers HTTP but its storage/source is incompatible, or the dashboard cannot fetch its scoped persisted endpoint
- **THEN** readiness fails for that resource rather than reporting all services ready

### Requirement: Actual receiver-backed persisted acceptance
Acceptance SHALL require an isolated finite fictional exchange through the official receiver, its expected acknowledgment, matching Kafka output, durable curated evidence/progress and matching scoped API/dashboard results. Direct Kafka input, stale records and independently generated fixture results MUST NOT satisfy this boundary.

#### Scenario: Complete receiver fixture
- **WHEN** the pinned two-vehicle fixture sends observed counters 100.00→100.25→100.50 and 200.00→200.125→200.25 with explicit synthetic boundaries through the receiver
- **THEN** ACK and newly observed decoded Kafka records match the fixture, curated evidence/progress commit and scoped results show 0.5/0.25 kWh respectively
- **AND** the unreviewed sessions remain Unconfirmed with zero eligible/priced energy and visible holds
- **AND** the report identifies exact commit, pinned inputs, source epoch and tested boundaries

#### Scenario: ACK or stale output alone
- **WHEN** a run receives an ACK but a required Kafka record, durable disposition or scoped result is absent, or only a preceding run's records exist
- **THEN** acceptance exits unsuccessfully within the documented observation deadline

### Requirement: Scoped consistent persisted reads
Every persisted read and review SHALL validate the configured owner/vehicle pairing and full session identity. Results SHALL come from one coherent committed evidence/review/rate snapshot and include held sessions. No default cross-owner combined read SHALL be exposed. Scope checks MUST NOT be represented as authentication.

#### Scenario: Independent owners with matching connection aliases
- **WHEN** owner-a/vehicle-a and owner-b/vehicle-b have the same local connection name and interleaved evidence
- **THEN** each scoped result contains only its own sessions, exact energy, decisions and subtotal
- **AND** changing the UI's selected vehicle cannot mix the owners' data

#### Scenario: Wrong scope or review target
- **WHEN** a read/review uses an unknown alias, mismatched owner/vehicle or another vehicle's session
- **THEN** it fails with a bounded safe scope error without revealing the other scope's records or mutating either scope

#### Scenario: Concurrent evidence and result request
- **WHEN** ingestion commits new evidence while a result is requested
- **THEN** the response reflects a consistent old or new committed snapshot with matching provenance
- **AND** it does not mix old energy with new reviews or rates

### Requirement: Persistent explicit manual classification
Sessions SHALL default to Unconfirmed. A deliberate bounded scoped action SHALL persist a revision of Unconfirmed, SharedCharger or OtherCharger for the exact evidence reviewed. Review changes SHALL use expected evidence/review versions and MUST NOT clear quality flags, charge-type exclusions or pricing holds. Free-text reviewer notes and real identities SHALL NOT be collected.

#### Scenario: Selected complete session confirmed
- **WHEN** only vehicle-a's complete session is explicitly confirmed SharedCharger against current evidence/review revisions
- **THEN** its 0.5 kWh becomes eligible and can be quoted with covered rates after restart
- **AND** vehicle-b remains Unconfirmed with zero eligible/priced energy

#### Scenario: Confirmation of held evidence
- **WHEN** a session with missing boundaries, rollback, conflicting or DC/ambiguous evidence is classified SharedCharger
- **THEN** the existing quality/exclusion reasons remain and its energy/cost stays excluded from eligible/priced totals

#### Scenario: Stale update or changed evidence
- **WHEN** a review submits an outdated expected revision, or distinct late evidence changes a previously reviewed session
- **THEN** the outdated update fails without replacing the current review, or the retained earlier decision is visibly stale and applied eligibility reverts to Unconfirmed
- **AND** explicit current-evidence reconfirmation is required before eligibility can return

#### Scenario: Repeated identical evidence
- **WHEN** identical semantic evidence is redelivered or reordered after a valid review
- **THEN** its applicable review remains valid and energy/cost is not duplicated

### Requirement: Immutable exact rate provenance
Persisted results SHALL select an immutable version of the labeled public sample tariff with exact decimal rates, effective coverage, explicit resolved windows and clock-conversion provenance compatible with event time. Same-version changed contents or invalid/overlapping coverage MUST fail. Repricing SHALL not rewrite evidence or borrow nearby coverage.

#### Scenario: Exact covered sample pricing
- **WHEN** current reviews confirm both complete receiver sessions and explicit synthetic UTC windows cover them at the September sample rate 0.2773931020 USD/kWh
- **THEN** scoped exact costs are 0.1386965510000000 and 0.0693482755000000 USD and scoped once-rounded subtotals are $0.14 and $0.07
- **AND** every line retains rate version, bounds, exact rate/energy/cost and the public-sample/physically-unvalidated labels

#### Scenario: Clock mismatch and conflicting rate version
- **WHEN** rate windows use incompatible synthetic ticks rather than the recorded event-time conversion, overlap, or reuse a version for changed contents
- **THEN** the configuration is refused safely without modifying retained evidence/reviews/rates

#### Scenario: Observed time outside dated coverage
- **WHEN** an observed synthetic record lies outside the selected sample rate version's effective dates or resolved window coverage
- **THEN** its positive energy remains held for missing coverage
- **AND** neither event timestamps nor rate effective dates are shifted to make it quote

#### Scenario: New version reprices preserved evidence
- **WHEN** a separately selected valid immutable rate revision covers the unchanged evidence
- **THEN** new results use only that revision and retain its provenance while observed energy, evidence and classification remain unchanged
- **AND** replay under the same selected revision reproduces the same exact quote

### Requirement: Conservative Rust-derived pricing and visible holds
Persisted costs SHALL use existing exact Rust domain pricing and aggregate exact amounts before final subtotal rounding. The frontend SHALL only present returned amounts. Missing rates/readings, unresolved boundaries and quality/exclusion reasons SHALL remain visible; held sessions MUST NOT receive partial, interpolated or invented-zero quotes.

#### Scenario: Missing rate or boundary sample
- **WHEN** positive energy lacks rate coverage or crosses a rate boundary without an observed boundary counter
- **THEN** the entire session has an explicit missing-rate or unresolved-boundary hold and no quoted cost
- **AND** known observed energy remains distinguishable from complete eligible/priced totals

#### Scenario: Held subtotal and valid zero
- **WHEN** all sessions in a scope are held
- **THEN** the view says Needs review/incomplete and counts the holds rather than presenting zero owed
- **WHEN** a complete confirmed session has unchanged counters
- **THEN** the existing core's valid zero-consumption quote can be shown explicitly without inventing a rate or claiming payment

#### Scenario: Exact aggregation and overflow
- **WHEN** several eligible lines/sessions have subcent costs, or exact addition overflows
- **THEN** successful subtotals round the exact aggregate once, or pricing fails without a partial/wrapped subtotal
- **AND** frontend display rounding never becomes calculation input

### Requirement: Reproducible versioned result snapshots
Every result SHALL carry its scoped semantic evidence, review, rate and calculation versions plus relevant source/mapping/manifest/normalizer provenance. Unchanged persisted evidence and applicable decisions SHALL reproduce identical session IDs, energy, holds and exact costs after restart/replay. New evidence or decisions MUST invalidate any current-result cache.

#### Scenario: Durable unchanged replay
- **WHEN** the runtime and API restart or retained deliveries are force-reread with unchanged semantic evidence, reviews and rates
- **THEN** scoped session identities, classifications, energy, holds and exact costs match the preceding snapshot with no double counting
- **AND** delivery progress/provenance changes cannot be mistaken for additional energy

#### Scenario: Late sample and conflict
- **WHEN** an observed late boundary sample resolves an interval or a distinct same-position conflicting variant arrives
- **THEN** a new scoped semantic revision reflects the current core reconstruction and stale review policy
- **AND** after an explicit applicable review the resolved interval can quote, while conflicts remain held and the other vehicle is unaffected
- **AND** replay of the final retained set in different arrival orders reproduces the same domain results

### Requirement: Owned state-preserving stop and restart
Documented stop/restart SHALL control only this checkout's supervisor, containers and host processes. Stop SHALL release listeners and writer locks while preserving broker state, epoch, database, reviews and rate revisions. Restart SHALL verify source and schema compatibility before resuming durable progress; silent reset or seek-to-latest is forbidden.

#### Scenario: Ordinary restart
- **WHEN** the documented restart command follows a successful reviewed calculation
- **THEN** all resources return to readiness and scoped semantic results/reviews/rates remain unchanged without resending counted energy

#### Scenario: Real process crash
- **WHEN** the actual owning runtime is terminated after reading/before commit or after commit/before local completion
- **THEN** restart resumes from consistent SQLite evidence/progress, accepting uncommitted input once or preserving committed input
- **AND** forced redelivery does not inflate scoped energy/cost

#### Scenario: Indeterminate commit and broker interruption
- **WHEN** process termination overlaps a commit window or the broker interrupts delivery
- **THEN** recovered evidence/progress is atomically old or new, and reconnect/replay follows its durable position
- **AND** the report identifies tested synchronization/guarantees without claiming instruction-level timing or broker-wide exactly-once delivery

### Requirement: Explicit narrow reset and recovery refusal
Reset SHALL require stopped owned resources, exact synthetic ownership verification and an explicit confirmation flag. It SHALL remove only the named checkout's synthetic broker/runtime/database companions and establish a new epoch on subsequent startup. Source mismatch, retention gaps and incompatible/corrupt schema SHALL fail without deleting state or presenting complete current results.

#### Scenario: Active or unowned reset
- **WHEN** reset is requested while an owned process/listener is active, without its confirmation flag, or against mismatched/symlinked/unowned state
- **THEN** it refuses without deletion and unrelated checkouts/volumes/files remain unchanged

#### Scenario: Clean isolated reset
- **WHEN** stopped owned synthetic state is explicitly reset and started again
- **THEN** a new source epoch and fresh database are created with Unconfirmed reviews
- **AND** two clean finite acceptance runs reproduce identical semantic fixture expectations

#### Scenario: Retention gap, broker reset or unsupported schema
- **WHEN** the durable next position is outside retained offsets, the broker epoch changes, or schema/configuration is unsupported
- **THEN** runtime readiness/acceptance fails with the relevant safe recovery category, preserving the prior database
- **AND** it does not auto-delete, seek newest input or label prior cached results current

### Requirement: Persisted and fixture UI separation
Persisted mode SHALL fetch scoped persisted endpoints and display synthetic review results, uncertainty, versions and last committed evidence time. Fixture mode SHALL retain its separate sample API/scenarios. Persisted failure MUST NOT fall back to fixture data or unsupported battery/status/calendar values. Reviews/rates SHALL not be stored as browser preferences.

#### Scenario: Inspect persisted results
- **WHEN** a user opens the persisted dashboard on desktop or at 375px mobile width
- **THEN** scoped session energy, eligible/priced totals, held counts, review/stale status and expandable exact-rate provenance match its actual persisted API response
- **AND** filters, deliberate review controls and details work by keyboard and tap without horizontal overflow or hidden essential hold information

#### Scenario: Persisted API unavailable
- **WHEN** persisted storage/API becomes unavailable or incompatible
- **THEN** the dashboard displays a safe unavailable/recovery state without substituting a fixture amount or pretending an earlier snapshot is current

#### Scenario: Fixture preview retained
- **WHEN** the existing direct preview or fixture URL is opened
- **THEN** its original complete/missing/winter sample scenarios and display preferences remain deterministic and explicitly fixture-labeled
- **AND** loading them neither writes persisted reviews nor changes the persisted evidence/rate selection

### Requirement: Private synthetic diagnostics and local exposure
Kafka SHALL remain private in the reviewed network. API/dashboard host publication/listeners SHALL use only IPv4 loopback; container-internal communication SHALL stay within the reviewed private project network. Runtime state and diagnostics SHALL retain only allowlisted fictional evidence, fixed reason codes and bounded version/stage summaries. Keys, certificates, raw payloads, locations and arbitrary rejected text MUST NOT enter public logs/artifacts. No live account access is implied.

#### Scenario: Private local graph
- **WHEN** resolved configuration and actual listeners/mount ownership are inspected
- **THEN** no Kafka host port/public API listener or unrestricted application mount exists, and DB/WAL/SHM files remain private under the runtime owner's UID

#### Scenario: Sensitive-looking rejected fields and browser mutation
- **WHEN** rejected input carries unexpected free text/location/credential-like content, or an unexpected browser origin attempts a review update
- **THEN** no supplied sensitive-looking text is present in storage/log/artifact output, and the unexpected-origin mutation is refused without changing a review

### Requirement: Dedicated bounded full-path verification
A dedicated fresh Linux x86_64 CI gate SHALL exercise the complete receiver-backed persisted path and result recovery with pinned local inputs, finite fixtures and hard deadlines. Setup/readiness/assertion/timeout/cleanup failures SHALL fail the gate. It SHALL always tear down owned resources and retain only allowlisted synthetic summaries. Manual desktop/mobile smoke SHALL be separately recorded.

#### Scenario: Wrong expectation fails then restored run passes
- **WHEN** an expected persisted energy/cost or output is deliberately wrong
- **THEN** the gate produces a verified failed run and still tears down owned resources
- **WHEN** the correct expectation is restored
- **THEN** a separate passing exact-commit run is required before claiming the full path verified

#### Scenario: Setup failure and safe teardown
- **WHEN** setup, assertion, timeout or signal interrupts the job
- **THEN** its failure status survives diagnostic pipelines and cleanup, only its own resources/state are stopped/reset, and any cleanup failure is visible
- **AND** retained artifacts contain no unrestricted runtime logs, database dumps or test trust material

#### Scenario: UI verification remains separate
- **WHEN** automated API/build checks pass but manual persisted desktop/mobile smoke has not run
- **THEN** its task remains unchecked and the report does not claim those UI interactions were verified
