# Synthetic telemetry harness delta

## Purpose

Verify the local transport boundary from synthetic vehicle messages through the official Tesla receiver into Kafka before building ChargeShare ingestion adapters.

## ADDED Requirements

### Requirement: Exercise the actual receiver boundary
The harness SHALL send fixed fictional telemetry through the pinned official receiver's authenticated transport and verify decoded output from Kafka. Directly producing fixture JSON to Kafka MUST NOT substitute for the receiver acceptance test. Only the local test certificate authority SHALL be trusted by the synthetic client and receiver.

#### Scenario: Successful receiver delivery
- **WHEN** the developer explicitly runs the documented smoke scenario against a ready environment
- **THEN** the sender completes its expected exchange and the verifier observes matching records from the receiver's Kafka dispatcher within a bounded timeout
- **AND** success identifies the scenario, pinned receiver version and verified boundary

#### Scenario: Reject invalid client authentication
- **WHEN** a test client presents no certificate or a certificate outside the generated test trust chain
- **THEN** the receiver rejects the connection and the verifier observes no corresponding telemetry record
- **AND** diagnostics exclude certificate private keys and arbitrary rejected payload content

### Requirement: Deterministic synthetic replay
The harness SHALL use versioned fixtures with fixed event times, fictional identities, values and expected output. It SHALL isolate each verification run from stale Kafka records and compare stable semantic fields rather than receipt times, offsets or cross-partition delivery order. Repeated clean runs MUST produce the same normalized result.

#### Scenario: Repeat a known scenario
- **WHEN** the developer resets and runs the same scenario twice with the same pinned inputs
- **THEN** both reports match the same expected per-vehicle records and outcome
- **AND** runtime-generated identifiers or timestamps do not create false differences

#### Scenario: Stale broker records
- **WHEN** old scenario records exist before a new run
- **THEN** the verifier uses a recorded start boundary and run isolation so old records cannot satisfy the new run

### Requirement: Preserve missing and adverse evidence
The harness SHALL distinguish complete delivery, intentionally omitted source fields, duplicate or out-of-order source messages, and transport failure. It MUST NOT invent readings, infer charging completion from silence, calculate reimbursements or claim domain deduplication from broker delivery.

#### Scenario: Missing reading fixture
- **WHEN** a scenario intentionally omits an energy reading
- **THEN** verification confirms the expected omission in decoded output without fabricating an energy value or cost

#### Scenario: Duplicate and out-of-order fixture
- **WHEN** a scenario sends a documented duplicate and reversed event-time sequence
- **THEN** verification compares the observed receiver output with version-specific expected behavior and preserves the fixed source event times
- **AND** the report makes no exactly-once or ledger-level correctness claim

### Requirement: Bounded failure and retention checks
The harness SHALL report which transport stage failed within documented timeouts. The Kafka verification path SHALL support reading retained synthetic records with an independent consumer position. Success MUST require expected output, not only a sender acknowledgment or a listening port.

#### Scenario: Broker unavailable
- **WHEN** Kafka is unavailable during a test
- **THEN** the run fails within its timeout with a broker or dispatch-stage diagnostic and never reports completed end-to-end delivery

#### Scenario: Missing expected output
- **WHEN** the sender completes but an expected record is absent at the verification deadline
- **THEN** the verifier exits unsuccessfully and names the missing fixture expectation without presenting a zero reading

#### Scenario: Replay retained output
- **WHEN** the test consumer is stopped and later restarted with an independent documented starting position while records remain within retention
- **THEN** retained synthetic records can be read again and compared against the expected output
- **AND** the report states that this checks local broker retention, not ChargeShare crash recovery or production durability
