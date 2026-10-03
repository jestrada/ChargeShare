# Vehicle ledger

## Purpose

Keep each vehicle's charging evidence, sessions and eligible AC energy separate,
and prove the accounting behavior entirely offline with synthetic data.

## ADDED Requirements

### Requirement: Explicit vehicle scope
The ledger SHALL require a configured vehicle alias for every event, session, review decision and total. Each vehicle SHALL belong to exactly one synthetic owner alias. Missing or unknown identities and duplicate vehicle registrations SHALL be rejected without changing any ledger state; diagnostics SHALL not echo rejected payloads.

#### Scenario: Two vehicles with interleaved events
- **WHEN** vehicle-a for owner-a and vehicle-b for owner-b report interleaved events
- **THEN** every derived session and total retains the correct vehicle and owner association, with no energy transferred between them

#### Scenario: Unknown identity
- **WHEN** an event lacks a vehicle alias or uses an unconfigured alias
- **THEN** it is rejected with a non-sensitive reason and neither vehicle's ledger changes

#### Scenario: Ambiguous registration
- **WHEN** the same vehicle alias is registered twice, including under different owners
- **THEN** configuration is rejected rather than selecting an owner implicitly

### Requirement: Vehicle-scoped reads and review
The offline domain interface SHALL require a vehicle scope for session reads, totals and review updates. A review target outside that scope SHALL fail without mutation. The core SHALL NOT return a combined cross-owner ledger by default; these scope checks SHALL NOT be described as authentication or production authorization.

#### Scenario: Review cannot affect another vehicle
- **WHEN** a review operation scoped to vehicle-a targets a session belonging to vehicle-b
- **THEN** the operation fails and vehicle-b's classification and totals remain unchanged

#### Scenario: Independent session lists
- **WHEN** sessions and totals are requested for vehicle-a
- **THEN** results contain only vehicle-a records even if another vehicle has matching timestamps and local session identifiers

### Requirement: Deterministic per-vehicle replay
The ledger SHALL deduplicate within vehicle scope and reconstruct results from event time with deterministic ordering. Replaying unchanged evidence and review decisions SHALL produce identical session identities, energy, flags and totals regardless of arrival order. Conflicting values at the same vehicle event position SHALL be flagged and held, not silently selected.

#### Scenario: Duplicate and late evidence
- **WHEN** a fixture is replayed with duplicate events and shuffled arrival order
- **THEN** its results equal the chronological replay and every energy delta contributes at most once

#### Scenario: Matching events across vehicles
- **WHEN** two vehicles have otherwise identical event values and timestamps
- **THEN** each retains its own evidence and energy; deduplication does not discard either vehicle's event

#### Scenario: Conflicting evidence
- **WHEN** one vehicle has different counter values for the same event position
- **THEN** the affected session is held for review, independent of arrival order, while the other vehicle remains unaffected

### Requirement: Conservative AC sessions
The ledger SHALL group pause/resume intervals within explicit synthetic connection boundaries for the same vehicle. Energy SHALL be the sum of valid nonnegative cumulative AC-counter deltas within a connection, labelled vehicle-reported AC energy with unvalidated physical accuracy. DC or ambiguous AC/DC evidence SHALL never be eligible as shared-charger AC energy.

#### Scenario: Independent connection and counter boundaries
- **WHEN** vehicle-a reports 0 to 10 kWh and vehicle-b reports 0 to 4 kWh in separate complete AC connections
- **THEN** their observed AC energy is 10 kWh and 4 kWh respectively, never a shared counter delta

#### Scenario: Pause and new connection
- **WHEN** a vehicle pauses and resumes without disconnecting, then starts a new explicit connection
- **THEN** the pause stays in the first session and the new connection starts a separate session without a delta across connections

#### Scenario: DC evidence
- **WHEN** an otherwise complete session has DC or ambiguous AC/DC evidence
- **THEN** it is excluded from eligible shared-charger AC energy with an explicit reason

### Requirement: Missing and invalid evidence remains visible
The ledger SHALL flag missing baseline, missing terminal sample, missing connection boundary, invalid or negative counter, counter rollback, and conflicting evidence. Such sessions SHALL be held out of eligible totals. Valid observed deltas SHALL remain distinguishable from a complete session total; silence SHALL NOT close a session or imply zero consumption.

#### Scenario: Bad counter and incomplete ending
- **WHEN** a counter decreases, is invalid, or a connection ends without a valid terminal sample
- **THEN** no negative or invented energy is counted and the session remains held with the corresponding quality flags

#### Scenario: Missing start or silence
- **WHEN** the first evidence lacks a valid connection-start baseline, or events stop without terminal evidence
- **THEN** the session remains incomplete and no baseline, end time or missing energy is fabricated

### Requirement: Separate energy accounting and manual eligibility
Every session SHALL default to unconfirmed charger classification. Per-vehicle summaries SHALL distinguish observed AC kWh, eligible shared-charger kWh, and held or excluded sessions with reasons. Only complete AC sessions explicitly marked shared-charger SHALL enter eligible kWh. Changing the charger classification SHALL NOT clear evidence-quality flags or finalize reimbursement.

#### Scenario: Independent confirmation
- **WHEN** complete sessions contain 10 kWh for vehicle-a and 4 kWh for vehicle-b and only vehicle-a is confirmed shared-charger
- **THEN** eligible totals are 10 kWh for vehicle-a and 0 kWh for vehicle-b; the latter remains unconfirmed

#### Scenario: Confirmation cannot override incomplete evidence
- **WHEN** a held session is marked shared-charger
- **THEN** it stays excluded from eligible kWh until evidence is complete; no amount due or payable statement is produced

### Requirement: Offline synthetic boundary
The milestone SHALL run without network, credentials or live vehicle input and SHALL use fictional owner and vehicle aliases in tests. Outputs SHALL identify themselves as synthetic review results, contain no locations or account identifiers, and make no utility-meter accuracy or production access-control claim.

#### Scenario: Run without Tesla access
- **WHEN** the offline test suite runs with no Tesla credentials or network access
- **THEN** all acceptance examples can be evaluated using repository-safe fixtures alone
