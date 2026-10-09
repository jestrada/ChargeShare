# Durable telemetry ingestion delta

## Purpose

Preserve fictional receiver evidence and consumer progress together so the existing vehicle-scoped ledger can be reconstructed deterministically after redelivery or a local process restart.

## ADDED Requirements

### Requirement: Verified synthetic receiver input
Ingestion SHALL consume the pinned stage-1 receiver's decoded Kafka output under a versioned input contract. Successful receiver acknowledgment MUST NOT mean durable application acceptance. The acceptance path MUST exercise the actual receiver; direct broker input SHALL be limited to focused consumer tests and labeled accordingly.

#### Scenario: Actual receiver acceptance
- **WHEN** a fixed two-vehicle fixture passes through the official receiver into Kafka and ingestion commits its accepted evidence
- **THEN** the report identifies the tested commit, pinned receiver/schema, manifest version and durable application boundary
- **AND** receiver acknowledgment and Kafka observation alone cannot satisfy ingestion acceptance

#### Scenario: Unsupported decoded shape
- **WHEN** a record has an unsupported schema, ambiguous duplicate source fields or an invalid source timestamp
- **THEN** it creates no normalized ledger event and records a bounded safe rejection disposition
- **AND** the report exposes the rejection without echoing arbitrary payload fields

### Requirement: Explicit synthetic identity scope
Ingestion SHALL require an unambiguous configured fictional device-to-vehicle/owner mapping. The broker key and decoded identity MUST agree. Missing, unknown, disagreeing or ambiguous identities MUST NOT mutate normalized evidence for any vehicle; diagnostics and rejection storage MUST NOT retain the rejected identity or payload.

#### Scenario: Interleaved configured vehicles
- **WHEN** fictional device-a maps to vehicle-a/owner-a and device-b maps to vehicle-b/owner-b and their records interleave
- **THEN** retained accepted evidence and reconstructed sessions remain associated with their mapped vehicle and owner

#### Scenario: Unknown or disagreeing identity
- **WHEN** identity is missing, unconfigured or disagrees between broker key and decoded record
- **THEN** neither vehicle's normalized evidence changes
- **AND** only source coordinates and a safe rejection code are retained

#### Scenario: Ambiguous configuration
- **WHEN** a device maps to multiple vehicles or a vehicle is registered more than once
- **THEN** startup rejects configuration before consuming or mutating existing evidence

### Requirement: Explicit synthetic boundary provenance
A versioned synthetic manifest SHALL supply stable vehicle-wide event positions, connection aliases, boundary kinds and charge-type annotations absent from receiver output. Each observed record MUST associate unambiguously by configured device and fixed source time. Manifest annotations SHALL remain distinct from receiver-measured values. Silence, transport reconnect or charge-complete state MUST NOT create an end boundary.

#### Scenario: Complete annotated connections
- **WHEN** observed baseline and terminal records match explicit Start and End annotations for one connection per vehicle
- **THEN** the adapter supplies those annotated boundaries to the existing core while retaining their synthetic provenance
- **AND** counters and timestamps come from observed receiver evidence, never an expected-results table

#### Scenario: Missing or ambiguous association
- **WHEN** an observed record has no unique manifest association or the manifest assigns one source association to contradictory connections or positions
- **THEN** the record is rejected safely or startup rejects the contradictory manifest before consumption
- **AND** arrival order, broker offset and wall-clock time are not used as replacement domain identities

#### Scenario: Missing terminal evidence
- **WHEN** a manifest describes an End but its corresponding record never arrives, or a charging-complete record has no End annotation
- **THEN** no manifest-only event or inferred End is created
- **AND** the core preserves an incomplete session with missing boundary or terminal evidence

### Requirement: Exact and missing counter fidelity
Normalization SHALL preserve exact supported decimal kWh through the existing core parser without floating-point arithmetic or rounding. Absent counters SHALL remain absent. Known scoped invalid counters SHALL retain safe rejection categories as event evidence. Missing boundary readings, rollback and conflicts SHALL preserve existing core flags; ingestion MUST NOT invent zeroes, interpolate readings or change the core's treatment of interior omissions.

#### Scenario: Decimal limits and invalid values
- **WHEN** observed counters include six fractional digits, a negative value, excess precision or overflow
- **THEN** valid decimals retain their exact value and invalid counters retain the existing corresponding rejection category
- **AND** unsafe rejected text is not retained or logged

#### Scenario: Omitted interior reading
- **WHEN** an AC Sample record lacks a counter between otherwise valid boundary readings
- **THEN** its absence remains visible in retained evidence and the event keeps an absent counter
- **AND** replay follows current core semantics without inventing an extra quality flag or measurement

#### Scenario: Missing boundary counter or rollback
- **WHEN** a Start or End lacks a counter, or a counter decreases within a connection
- **THEN** existing missing-baseline, missing-terminal or rollback flags remain visible and the affected session is held from eligible totals

### Requirement: Durable evidence and progress are atomic
For each consumed record, accepted evidence and normalized variants or a safe rejection disposition SHALL commit atomically with that partition's next resume position. Automatic broker progress MUST NOT outrun durable evidence. Known rollback MUST preserve both; an indeterminate commit SHALL recover consistent old-or-new state. Recovery SHALL use the durable position rather than an independent broker checkpoint.

#### Scenario: Crash before commit
- **WHEN** the process stops after reading a record or writing transaction-local rows but before commit
- **THEN** restart reconsumes from the prior durable position and accepts the record without losing evidence or counting energy twice

#### Scenario: Crash during commit
- **WHEN** the process stops while commit completion is indeterminate
- **THEN** reopened storage contains either the prior evidence/progress or the committed evidence/progress atomically
- **AND** restart follows that recovered position without lost accepted evidence or duplicate energy

#### Scenario: Crash after commit
- **WHEN** the process stops after committing evidence and next position but before acknowledging local processing completion
- **THEN** restart retains that evidence and resumes from the committed position
- **AND** forced redelivery of the committed record is idempotent

#### Scenario: Storage failure or competing writer
- **WHEN** storage is unavailable, full, corrupt or locked beyond the bounded deadline, or another writer owns this runtime database
- **THEN** ingestion fails with a safe storage or ownership reason and does not advance durable progress past uncommitted input

#### Scenario: Rejected record does not block forever
- **WHEN** a malformed or unknown-identity record receives a durable safe rejection disposition
- **THEN** its source position and rejection commit together so the consumer can continue without repeatedly logging its contents
- **AND** it never becomes accepted vehicle evidence or a successful complete-fixture result

### Requirement: Delivery identity and domain identity stay separate
Ingestion SHALL distinguish stream/topic/partition/offset delivery identity from normalized event identity. Redelivery MUST NOT duplicate accepted evidence. Distinct normalized variants at the same vehicle-wide event position MUST all remain replayable, including variants associated with different connections. Matching events from different vehicles MUST remain separate.

#### Scenario: Duplicate transport and source events
- **WHEN** the same delivery is reread or identical domain evidence is delivered at new broker offsets
- **THEN** delivery dispositions remain auditable but each unique normalized event contributes at most once to domain replay

#### Scenario: Conflict across connections
- **WHEN** one vehicle has different normalized events at the same position, including different connection aliases
- **THEN** every variant is retained and replay produces the core's conflict barriers and holds for all affected connections, independent of arrival order
- **AND** another vehicle using that position remains unaffected

#### Scenario: Changed contents at an existing delivery coordinate
- **WHEN** the same bound stream/topic/partition/offset is presented with different accepted semantic contents
- **THEN** ingestion fails without overwriting its earlier disposition or silently advancing progress

### Requirement: Deterministic scoped durable replay
Reconstruction SHALL register frozen synthetic mappings and replay all persisted normalized variants through the existing core. Unchanged evidence SHALL reproduce identical vehicle/owner associations, session IDs, observed AC energy and quality/exclusion reasons after reordered arrival, reconnect and restart. Sessions SHALL default to unconfirmed classification; this stage MUST NOT persist review decisions or produce payable amounts.

#### Scenario: Independent ten and four kWh
- **WHEN** complete annotated AC evidence records vehicle-a counters 0 to 10 kWh and vehicle-b counters 0 to 4 kWh
- **THEN** scoped reconstruction yields observed AC totals of 10 and 4 kWh respectively before and after restart
- **AND** both remain unconfirmed with zero eligible shared-charger kWh

#### Scenario: Late evidence and pause resume
- **WHEN** delayed records complete a known connection and Pause/Resume annotations occur before another explicit connection
- **THEN** replay uses source time with existing deterministic tie ordering, keeps the pause in its connection and never bridges counters across connections
- **AND** shuffled replay of the final evidence set yields the same results

#### Scenario: Scoped read rejects another vehicle
- **WHEN** replay results are requested under vehicle-a's configured owner/vehicle scope
- **THEN** only vehicle-a results are returned and an unknown or mismatched scope fails without mutation

### Requirement: Compatible local storage lifecycle
Startup SHALL verify storage schema, frozen mapping/manifest/normalizer versions and source-stream identity before consumption. Existing evidence MUST NOT be silently reinterpreted, erased or connected to a reset topic. Out-of-range progress or expired retained input SHALL produce an explicit recovery failure rather than silently seek to newest data. Recovery guarantees SHALL identify the tested local storage boundary.

#### Scenario: Fresh database initialization
- **WHEN** a new isolated database starts against retained synthetic input
- **THEN** it requires and durably records an explicit retained starting boundary before consumption
- **AND** no implicit seek-to-latest skips existing fixture evidence

#### Scenario: Incompatible state
- **WHEN** an existing database uses a newer schema or changed mapping, manifest or normalization contract
- **THEN** startup fails without modifying its evidence and explains the compatibility category safely

#### Scenario: Broker reset or retention gap
- **WHEN** the source stream changes or the persisted next position is outside retained broker bounds
- **THEN** ingestion stops with a source-identity or recovery-gap error, preserving the database
- **AND** it does not reset itself, skip lost evidence or report a complete result

#### Scenario: Interrupted supported migration
- **WHEN** a documented supported migration fails or the process stops during its transaction
- **THEN** the prior recoverable schema/evidence remains intact, or startup reports an explicit unsupported/corrupt-state failure without deleting it

### Requirement: Private synthetic runtime and safe diagnostics
Ingestion SHALL run only against the isolated local synthetic stack, retain only allowlisted fictional evidence in ignored runtime storage and emit bounded stage/reason summaries. Unknown payloads, locations, account data, keys and credentials MUST NOT enter storage, logs or artifacts. Automated reset SHALL affect only its documented owned synthetic runtime; live collection and public ingress remain separately authorized work.

#### Scenario: Rejected sensitive-looking fields
- **WHEN** a malformed record contains an unexpected identity, free text, location or credential-looking value
- **THEN** storage/log/artifact inspection finds only permitted coordinates and rejection codes, with none of the supplied text

#### Scenario: Successful private synthetic run
- **WHEN** the acceptance suite runs on a fresh supported Linux environment without Tesla accounts or repository secrets
- **THEN** it uses the stage-1 isolated trust/network and reports synthetic, physically unvalidated review results
- **AND** the existing fixture dashboard remains independent of ingestion

### Requirement: Bounded automated recovery acceptance
A dedicated ingestion gate SHALL exercise the actual receiver, normalization, local persistence and scoped replay on supported Linux x86_64. It SHALL fail on setup, readiness, assertion, persistence or timeout failures and always tear down owned resources. Focused crash/malformed-input tests SHALL supplement, never replace, the receiver path. Only allowlisted synthetic summaries SHALL be retained.

#### Scenario: Successful recovery gate
- **WHEN** the gate receives fixed multi-vehicle evidence through the pinned receiver and injects before/after-commit restarts, duplicates, reordering and adverse evidence
- **THEN** it verifies durable positions, exact scoped totals, preserved holds and rejection privacy against the scenario expectations
- **AND** identifies the final commit and tested boundary without claiming API/dashboard or Cloudflare coverage

#### Scenario: Controlled failure and cleanup
- **WHEN** an expected record, total or recovery assertion is deliberately wrong, or setup fails
- **THEN** the gate exits unsuccessfully without masking failure and cleanup still runs
- **AND** a restored correct run is separately verified before recording implementation completion
